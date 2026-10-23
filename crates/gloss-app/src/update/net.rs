//! 更新子系统的网络边界：进程内唯一的 HTTP 客户端与清单拉取。
//!
//! 非 200、超限 body、非 UTF-8、校验矩阵不符都收敛为 `Err(())`——细节只
//! 进日志，状态机只认失败。

use std::sync::OnceLock;
use std::time::Duration;

use futures::StreamExt;

use gloss_core::log::{thread, warn};

use super::manifest::{self, UpdateManifest};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

pub(super) async fn fetch_manifest(
    client: &reqwest::Client,
    url: &str,
) -> Result<UpdateManifest, ()> {
    let response = client.get(url).send().await.map_err(|err| {
        warn!(thread = thread::TOKIO, error = %err, "update: manifest fetch failed");
    })?;
    let status = response.status();
    if status != reqwest::StatusCode::OK {
        warn!(thread = thread::TOKIO, status = %status, "update: manifest fetch not ok");
        return Err(());
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| {
            warn!(thread = thread::TOKIO, error = %err, "update: manifest body read failed");
        })?;
        if bytes.len() + chunk.len() > MAX_MANIFEST_BYTES as usize {
            warn!(thread = thread::TOKIO, "update: manifest body too large");
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|err| {
        warn!(thread = thread::TOKIO, error = %err, "update: manifest body is not utf-8");
    })?;
    manifest::parse(text).map_err(|err| {
        warn!(thread = thread::TOKIO, error = ?err, "update: manifest rejected");
    })
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) fn shared_client() -> Result<&'static reqwest::Client, reqwest::Error> {
    let client = match HTTP_CLIENT.get() {
        Some(client) => client,
        None => {
            let client = build_client()?;
            HTTP_CLIENT.get_or_init(|| client)
        }
    };
    Ok(client)
}

fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent(concat!("gloss/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}
