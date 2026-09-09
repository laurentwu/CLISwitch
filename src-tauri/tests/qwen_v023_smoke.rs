use std::{collections::BTreeMap, path::Path, time::Duration};

use chrono::Utc;
use cliswitch_lib::{
    adapters::{CliAdapter, HostEnvironment, QwenAdapter},
    domain::{
        ApiProviderData, CliId, CliProtocol, ConfigurationTarget, ConnectionAuthType,
        ProviderConnection, ProviderData, ProviderProfile, VerificationInfo,
    },
};
use serde_json::Value;
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use url::Url;
use uuid::Uuid;

const MODEL: &str = "fixture/qwen-model";

fn environment(home: &Path) -> HostEnvironment {
    HostEnvironment {
        home: home.to_path_buf(),
        variables: BTreeMap::new(),
        present_variables: Default::default(),
        os: std::env::consts::OS.into(),
    }
}

fn provider(endpoint: Url, api_key: &str) -> (ProviderProfile, ConfigurationTarget) {
    let now = Utc::now();
    let provider_id = Uuid::new_v4();
    let connection_id = Uuid::new_v4();
    (
        ProviderProfile {
            id: provider_id,
            name: "Official Qwen smoke provider".into(),
            template_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            data: ProviderData::Api(ApiProviderData {
                connections: vec![ProviderConnection {
                    id: connection_id,
                    template_endpoint_id: None,
                    credential_slot_id: "api-key".into(),
                    protocol: CliProtocol::OpenaiChat,
                    endpoint,
                    auth_type: ConnectionAuthType::Bearer,
                    api_key: api_key.into(),
                    default_model: MODEL.into(),
                    verification: VerificationInfo::default(),
                }],
            }),
        },
        ConfigurationTarget::Api {
            cli_id: CliId::Qwen,
            provider_id,
            connection_id,
            model: MODEL.into(),
        },
    )
}

async fn apply_generated_settings(
    adapter: &QwenAdapter,
    home: &Path,
    provider: &ProviderProfile,
    target: &ConfigurationTarget,
) {
    let host = environment(home);
    let paths = adapter.resolve_paths(&host, Some(home.to_path_buf()));
    let plan = adapter
        .plan_write(&paths, target, provider, &host)
        .await
        .unwrap();
    assert_eq!(plan.files.len(), 1);
    tokio::fs::create_dir_all(home).await.unwrap();
    tokio::fs::write(&paths.config_file, &plan.files[0].target_content)
        .await
        .unwrap();
    assert!(adapter.verify_applied(&plan).await.unwrap());
}

async fn read_http_request(stream: &mut TcpStream) -> (String, BTreeMap<String, String>, Value) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "official Qwen CLI closed the mock request early");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap().to_string();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect::<BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    while bytes.len() < header_end + content_length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "official Qwen CLI sent an incomplete mock body");
        bytes.extend_from_slice(&buffer[..read]);
    }
    let body = serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap();
    (request_line, headers, body)
}

async fn write_json_response(stream: &mut TcpStream, body: &Value) {
    let body = serde_json::to_vec(body).unwrap();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
}

async fn write_chat_response(stream: &mut TcpStream, stream_response: bool) {
    if !stream_response {
        write_json_response(
            stream,
            &serde_json::json!({
                "id": "chatcmpl-fixture",
                "object": "chat.completion",
                "created": 1,
                "model": MODEL,
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "fixture-ok" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
            }),
        )
        .await;
        return;
    }
    let chunks = [
        serde_json::json!({
            "id": "chatcmpl-fixture",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": MODEL,
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": "fixture-ok" },
                "finish_reason": null
            }]
        }),
        serde_json::json!({
            "id": "chatcmpl-fixture",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": MODEL,
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        }),
    ];
    let body = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        chunks[0], chunks[1]
    );
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    stream.write_all(header.as_bytes()).await.unwrap();
    stream
        .write_all(format!("{:X}\r\n", body.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    stream.write_all(b"\r\n0\r\n\r\n").await.unwrap();
}

async fn mock_openai(expected_requests: usize) -> (Url, oneshot::Receiver<Vec<(String, String)>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut observed = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (request_line, headers, body) = read_http_request(&mut stream).await;
            let path = request_line.split_whitespace().nth(1).unwrap_or_default();
            if path.ends_with("/models") {
                write_json_response(
                    &mut stream,
                    &serde_json::json!({ "object": "list", "data": [{ "id": MODEL }] }),
                )
                .await;
                continue;
            }
            assert!(path.ends_with("/chat/completions"));
            let authorization = headers.get("authorization").cloned().unwrap_or_default();
            let model = body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let stream_response = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
            write_chat_response(&mut stream, stream_response).await;
            observed.push((authorization, model));
            if observed.len() == expected_requests {
                let _ = sender.send(observed);
                break;
            }
        }
    });
    (
        Url::parse(&format!("http://{address}/v1")).unwrap(),
        receiver,
    )
}

async fn run_official_cli(binary: &Path, root: &TempDir, qwen_home: &Path) {
    let work = root.path().join("empty-workspace");
    let isolated_home = root.path().join("isolated-home");
    tokio::fs::create_dir_all(&work).await.unwrap();
    tokio::fs::create_dir_all(&isolated_home).await.unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        tokio::process::Command::new(binary)
            .args([
                "--telemetry=false",
                "--chat-recording=false",
                "--approval-mode=plan",
                "--max-session-turns=1",
                "--max-wall-time=30s",
                "--max-tool-calls=0",
                "--output-format=json",
                "Reply only with fixture-ok",
            ])
            .current_dir(work)
            .env("HOME", &isolated_home)
            .env("USERPROFILE", &isolated_home)
            .env("QWEN_HOME", qwen_home)
            .env_remove("OPENAI_API_KEY")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("official Qwen CLI timed out")
    .unwrap();
    let safe_stdout = String::from_utf8_lossy(&output.stdout)
        .replace("fixture-account-a-key", "[REDACTED]")
        .replace("fixture-account-b-key", "[REDACTED]");
    let safe_stderr = String::from_utf8_lossy(&output.stderr)
        .replace("fixture-account-a-key", "[REDACTED]")
        .replace("fixture-account-b-key", "[REDACTED]");
    assert!(
        output.status.success(),
        "official Qwen CLI failed with {}: stdout={safe_stdout}; stderr={safe_stderr}",
        output.status
    );
    assert!(safe_stdout.contains("fixture-ok"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires QWEN_V023_BIN pointing to @qwen-code/qwen-code 0.23.0"]
async fn generated_settings_work_with_official_qwen_v023_for_account_and_endpoint_switches() {
    let binary = std::env::var_os("QWEN_V023_BIN")
        .map(std::path::PathBuf::from)
        .expect("QWEN_V023_BIN is required");
    let root = tempfile::tempdir().unwrap();
    let qwen_home = root.path().join("qwen-home");
    let adapter = QwenAdapter;

    let (endpoint_a, observed_a) = mock_openai(4).await;
    let (provider_a, target_a) = provider(endpoint_a.clone(), "fixture-account-a-key");
    apply_generated_settings(&adapter, &qwen_home, &provider_a, &target_a).await;
    run_official_cli(&binary, &root, &qwen_home).await;

    let (mut provider_b, target_b) = provider(endpoint_a, "fixture-account-b-key");
    apply_generated_settings(&adapter, &qwen_home, &provider_b, &target_b).await;
    run_official_cli(&binary, &root, &qwen_home).await;
    let observed_a = observed_a.await.unwrap();
    assert_eq!(observed_a.len(), 4);
    assert!(
        observed_a[..2]
            .iter()
            .all(|observed| observed == &("Bearer fixture-account-a-key".into(), MODEL.into()))
    );
    assert!(
        observed_a[2..]
            .iter()
            .all(|observed| observed == &("Bearer fixture-account-b-key".into(), MODEL.into()))
    );
    let settings = tokio::fs::read_to_string(qwen_home.join("settings.json"))
        .await
        .unwrap();
    let settings: Value =
        jsonc_parser::parse_to_serde_value(&settings, &jsonc_parser::ParseOptions::default())
            .unwrap();
    assert_eq!(
        settings["modelProviders"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|models| models.as_array().unwrap())
            .filter(|model| model["id"] == MODEL)
            .count(),
        1
    );
    let (endpoint_b, observed_b) = mock_openai(2).await;
    // Rebind the same saved B connection to a distinct loopback route and verify model.baseUrl
    // selects it without removing the first route.
    let ProviderData::Api(api_b) = &mut provider_b.data else {
        unreachable!()
    };
    api_b.connections[0].endpoint = endpoint_b;
    apply_generated_settings(&adapter, &qwen_home, &provider_b, &target_b).await;
    run_official_cli(&binary, &root, &qwen_home).await;
    let observed_b = observed_b.await.unwrap();
    assert_eq!(observed_b.len(), 2);
    assert!(
        observed_b
            .iter()
            .all(|observed| observed == &("Bearer fixture-account-b-key".into(), MODEL.into()))
    );
}
