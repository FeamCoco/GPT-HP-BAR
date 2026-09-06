//! GPT-HP-BAR 数据采集层
//!
//! 数据源优先级（详见 docs/需求与技术方案.md §1.3 / §6）：
//!   1. rest       直连 ChatGPT 后端 wham/usage（Bearer = auth.json tokens.access_token）
//!   2. app-server 本机 `codex app-server` JSON-RPC（token 由 CLI 自续期）
//!   3. relay      中转：config.toml base_url + bearer/api-key 探测常见用量路径
//!   4. none       全部失败，UI 显示"无数据源"
//!
//! 安全约定：本模块绝不打印/记录任何 token，probe 输出只含状态与数值。

use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WHAM_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const UA: &str = "GPT-HP-BAR/0.1 (Windows; +local)";

#[derive(Serialize, Clone, Default, Debug)]
pub struct WindowUsage {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_in_seconds: Option<i64>,
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct Usage {
    pub ok: bool,
    pub source: String,
    pub plan: Option<String>,
    pub email: Option<String>,
    pub credits: Option<Value>,
    pub primary: WindowUsage,
    pub secondary: WindowUsage,
    pub error: Option<String>,
    pub fetched_at: u64,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn codex_home() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_HOME") { return PathBuf::from(p); }
    let mut p = std::env::var("USERPROFILE").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    p.push(".codex");
    p
}

fn read_json_file(p: &PathBuf) -> Option<Value> {
    std::fs::read_to_string(p).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// auth.json 三种形态：OAuth(tokens.access_token) / 纯 API key / null
struct Auth {
    access_token: Option<String>,
    api_key: Option<String>,
    id_token: Option<String>,
}

fn read_auth() -> Option<Auth> {
    let v = read_json_file(&codex_home().join("auth.json"))?;
    let tokens = v.get("tokens");
    Some(Auth {
        access_token: tokens.and_then(|t| t.get("access_token")).and_then(|x| x.as_str()).map(String::from),
        api_key: v.get("OPENAI_API_KEY").and_then(|x| x.as_str())
            .filter(|s| !s.is_empty() && *s != "null").map(String::from),
        id_token: tokens.and_then(|t| t.get("id_token")).and_then(|x| x.as_str()).map(String::from),
    })
}

/// config.toml：按顶层 model_provider 选激活的 provider，取其 base_url / bearer（值绝不外泄）
fn read_config() -> Option<(Option<String>, Option<String>)> {
    let s = std::fs::read_to_string(codex_home().join("config.toml")).ok()?;
    let v: toml::Value = s.parse().ok()?;
    let active = v.get("model_provider").and_then(|x| x.as_str());
    let providers = v.get("model_providers").and_then(|x| x.as_table());

    let mut base = v.get("base_url").and_then(|x| x.as_str()).map(String::from);
    let mut bearer = v.get("experimental_bearer_token").and_then(|x| x.as_str()).map(String::from);

    if base.is_none() || bearer.is_none() {
        let pick = active.and_then(|n| providers.and_then(|t| t.get(n)))
            .or_else(|| providers.and_then(|t| t.values().find(|p| p.get("base_url").is_some())));
        if let Some(p) = pick {
            if base.is_none() {
                base = p.get("base_url").and_then(|x| x.as_str()).map(String::from);
            }
            if bearer.is_none() {
                bearer = p.get("experimental_bearer_token").and_then(|x| x.as_str()).map(String::from);
            }
        }
    }
    Some((base, bearer))
}

/// 极简 base64url 解码（JWT payload），失败返回 None
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    const TBL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' { break; }
        let v = TBL.iter().position(|t| *t == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 { bits -= 8; out.push(((buf >> bits) & 0xFF) as u8); }
    }
    Some(out)
}

/// 从 id_token JWT 提取 plan 与邮箱
fn identity_from_id_token(id_token: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 { return (None, None); }
    let payload = b64url_decode(parts[1]).and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let Some(p) = payload else { return (None, None) };
    let auth = p.get("https://api.openai.com/auth");
    let plan = auth.and_then(|a| a.get("chatgpt_plan_type")).and_then(|x| x.as_str()).map(String::from)
        .or_else(|| p.get("chatgpt_plan_type").and_then(|x| x.as_str()).map(String::from));
    let email = p.get("email").and_then(|x| x.as_str()).map(String::from)
        .or_else(|| auth.and_then(|a| a.get("email")).and_then(|x| x.as_str()).map(String::from));
    (plan, email)
}

fn http_get_json(url: &str, bearer: &str, account_id: Option<&str>) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(UA)
        .build().map_err(|e| e.to_string())?;
    let mut req = client.get(url).bearer_auth(bearer)
        .header("Accept", "application/json");
    if let Some(acc) = account_id { req = req.header("chatgpt-account-id", acc); }
    let resp = req.send().map_err(|e| format!("{}: {}", url, e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("{} -> HTTP {}", url, status.as_u16()));
    }
    resp.json().map_err(|e| format!("{} 解析失败: {}", url, e))
}

/// 宽容解析 wham/usage 响应：rate_limits | rate_limit 两种拼写都认
fn parse_wham(v: &Value) -> (WindowUsage, WindowUsage, Option<Value>) {
    let rl = v.get("rate_limits").or_else(|| v.get("rate_limit")).unwrap_or(&Value::Null);
    let win = |key: &str| {
        let w = rl.get(key).unwrap_or(&Value::Null);
        WindowUsage {
            used_percent: w.get("used_percent").and_then(|x| x.as_f64()),
            window_minutes: w.get("window_minutes").and_then(|x| x.as_u64()),
            resets_in_seconds: w.get("resets_in_seconds").and_then(|x| x.as_i64())
                .or_else(|| w.get("resets_in_minutes").and_then(|x| x.as_i64()).map(|m| m * 60)),
        }
    };
    let credits = v.get("credits").cloned()
        .or_else(|| v.get("credits_balance").map(|b| serde_json::json!({ "balance": b })));
    (win("primary_window"), win("secondary_window"), credits)
}

fn fetch_rest(auth: &Auth, account_id: Option<&str>) -> Result<Usage, String> {
    let token = auth.access_token.as_ref().ok_or("rest: auth.json 无 tokens.access_token（非 ChatGPT OAuth 登录）")?;
    let v = http_get_json(WHAM_URL, token, account_id)?;
    let (primary, secondary, credits) = parse_wham(&v);
    Ok(Usage {
        ok: true, source: "rest".into(),
        credits,
        primary, secondary,
        fetched_at: now_unix(),
        ..Default::default()
    })
}

/// codex app-server JSON-RPC（stdin/stdout JSONL），尽力而为
fn fetch_app_server() -> Result<Usage, String> {
    // GUI 进程拉起控制台程序会闪黑窗，必须加 CREATE_NO_WINDOW
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let args = ["-s", "read-only", "-a", "never", "app-server"];
    let child = Command::new("codex").args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn()
        .or_else(|_| {
            // npm 全局安装的是 codex.cmd，需经 cmd 解析
            Command::new("cmd").args(["/C", "codex"]).args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
                .spawn()
        })
        .map_err(|e| format!("app-server: 无法启动 codex ({})", e))?;
    let mut child = child;

    let mut stdin = child.stdin.take().ok_or("app-server: stdin 不可用")?;
    let stdout = child.stdout.take().ok_or("app-server: stdout 不可用")?;

    // 后台读线程：把 stdout 按行推入 channel，主线程用超时收
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) { if tx.send(line).is_err() { break; } }
    });

    let send = |stdin: &mut std::process::ChildStdin, v: Value| {
        writeln!(stdin, "{}", v).map_err(|e| e.to_string())
    };
    send(&mut stdin, serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "clientInfo": { "name": "GPT-HP-BAR", "version": "0.1.0" } }
    }))?;
    send(&mut stdin, serde_json::json!({"jsonrpc":"2.0","method":"initialized"})).ok();
    send(&mut stdin, serde_json::json!({"jsonrpc":"2.0","id":2,"method":"account/read"}))?;
    send(&mut stdin, serde_json::json!({"jsonrpc":"2.0","id":3,"method":"account/rateLimits/read"}))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut rate: Option<Value> = None;
    let mut account: Option<Value> = None;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(line) => {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                match v.get("id").and_then(|i| i.as_u64()) {
                    Some(2) => account = v.get("result").cloned(),
                    Some(3) => rate = v.get("result").cloned(),
                    _ => {}
                }
                if rate.is_some() { break; }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        }
    }
    let _ = child.kill();

    let Some(rate) = rate else { return Err("app-server: 未取得 rateLimits 结果".into()) };
    let (primary, secondary, credits) = parse_wham(&rate);
    let plan = account.as_ref().and_then(|a| a.get("plan")).and_then(|x| x.as_str()).map(String::from);
    let email = account.as_ref().and_then(|a| a.get("email")).and_then(|x| x.as_str()).map(String::from);
    Ok(Usage {
        ok: true, source: "app-server".into(), plan, email, credits,
        primary, secondary, fetched_at: now_unix(),
        ..Default::default()
    })
}

fn fetch_relay(base_url: &str, key: &str) -> Result<Usage, String> {
    let base = base_url.trim_end_matches('/');
    let paths = ["/backend-api/wham/usage", "/wham/usage"];
    let mut last_err = String::new();
    for p in paths {
        match http_get_json(&format!("{}{}", base, p), key, None) {
            Ok(v) => {
                let (primary, secondary, credits) = parse_wham(&v);
                return Ok(Usage {
                    ok: true, source: "relay".into(), credits,
                    primary, secondary, fetched_at: now_unix(),
                    ..Default::default()
                });
            }
            Err(e) => last_err = e,
        }
    }
    Err(format!("relay: {}", last_err))
}

/// 依次尝试全部数据源；同时从 auth.json 补充 plan/email 身份信息
pub fn fetch_all() -> Usage {
    let auth = read_auth();
    let config = read_config();
    let account_id = auth.as_ref().and_then(|a| a.id_token.as_ref())
        .and_then(|t| identity_from_id_token(t).1.clone());

    let mut attempted: Vec<String> = Vec::new();

    // 1. rest：直连 wham/usage（需要 ChatGPT OAuth 登录态）
    let mut usage = None;
    if let Some(a) = auth.as_ref() {
        if a.access_token.is_some() {
            match fetch_rest(a, account_id.as_deref()) {
                Ok(u) => usage = Some(u),
                Err(e) => attempted.push(e),
            }
        } else {
            attempted.push("rest: 无 access_token（非 OAuth 登录态）".into());
        }
    } else {
        attempted.push("rest: auth.json 不可读".into());
    }

    // 2. app-server：本机 codex CLI JSON-RPC
    if usage.is_none() {
        match fetch_app_server() {
            Ok(u) => usage = Some(u),
            Err(e) => attempted.push(e),
        }
    }

    // 3. relay：config.toml base_url + bearer/api-key
    if usage.is_none() {
        let base = config.as_ref().and_then(|c| c.0.clone());
        let key = config.as_ref().and_then(|c| c.1.clone())
            .or_else(|| auth.as_ref().and_then(|a| a.api_key.clone()));
        if let (Some(b), Some(k)) = (base, key) {
            match fetch_relay(&b, &k) {
                Ok(u) => usage = Some(u),
                Err(e) => attempted.push(e),
            }
        } else {
            attempted.push("relay: 无 base_url 或密钥".into());
        }
    }

    let mut usage = usage.unwrap_or_else(|| Usage {
        ok: false, source: "none".into(),
        error: Some(attempted.join("；")),
        fetched_at: now_unix(),
        ..Default::default()
    });

    // 身份信息（任何来源都可能没有，从本地 auth 补齐）
    if let Some(a) = auth {
        if usage.plan.is_none() || usage.email.is_none() {
            if let Some(t) = a.id_token {
                let (plan, email) = identity_from_id_token(&t);
                usage.plan = usage.plan.or(plan);
                usage.email = usage.email.or(email);
            }
        }
    }
    usage
}

/// `--probe` 模式：打印数据源探测结果（绝不输出密钥）
pub fn probe() {
    println!("GPT-HP-BAR 数据源探测");
    println!("CODEX_HOME = {}", codex_home().display());
    let auth = read_auth();
    let config = read_config();
    println!("auth.json: access_token={}, api_key={}, id_token={}",
        auth.as_ref().map_or("无", |a| if a.access_token.is_some() {"有"} else {"无"}),
        auth.as_ref().map_or("无", |a| if a.api_key.is_some() {"有"} else {"无"}),
        auth.as_ref().map_or("无", |a| if a.id_token.is_some() {"有"} else {"无"}));
    match &config {
        Some((base, bearer)) => println!("config.toml: base_url={:?} bearer={}", base.as_deref().unwrap_or("(未设置)"),
            if bearer.is_some() {"有(值不打印)"} else {"无"}),
        None => println!("config.toml: 未读取到 base_url/bearer"),
    }
    if let Some(a) = &auth {
        if let Some(t) = &a.id_token {
            let (plan, email) = identity_from_id_token(t);
            println!("身份(JWT): plan={:?} email={:?}", plan, email);
        }
    }
    println!("\n-- fetch_all 综合结果 --");
    let u = fetch_all();
    println!("ok={} source={}", u.ok, u.source);
    println!("plan={:?} email={:?}", u.plan, u.email);
    println!("primary: used={:?} window_min={:?} reset_in={:?}", u.primary.used_percent, u.primary.window_minutes, u.primary.resets_in_seconds);
    println!("secondary: used={:?} window_min={:?} reset_in={:?}", u.secondary.used_percent, u.secondary.window_minutes, u.secondary.resets_in_seconds);
    println!("credits={:?}", u.credits.is_some());
    if let Some(e) = &u.error { println!("error: {}", e); }
}
