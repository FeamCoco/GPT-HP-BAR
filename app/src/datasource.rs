//! GPT-HP-BAR 数据采集层
//!
//! 数据源优先级（详见 docs/需求与技术方案.md §1.3 / §6）：
//!   1. rest       直连 ChatGPT 后端 wham/usage（Bearer = auth.json tokens.access_token）
//!   2. app-server 本机 `codex app-server` JSON-RPC（token 由 CLI 自续期）
//!   3. relay      中转：config.toml base_url + bearer/api-key 探测常见用量路径
//!   4. none       全部失败，UI 显示"无数据源"
//!
//! 安全约定：本模块绝不打印/记录任何 token，probe 输出只含状态与数值。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WHAM_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const UA: &str = "GPT-HP-BAR/0.1 (Windows; +local)";

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct WindowUsage {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_in_seconds: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
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
    /// 数据源状态（前端据此给出各自的下一步引导）：
    /// `ok` | `no_login` | `no_relay` | `network` | `none`
    /// `#[serde(default)]` 保证老消费者读到缺失字段时退化为空串而非报错。
    #[serde(default)]
    pub status: String,
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

/// 宽容数值提取：int/float 都收（serde 的 as_i64 遇到 4520.0 这类浮点会返回 None，
/// 曾导致倒计时永远显示 "--"）；字符串数字也兜底
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('%').parse::<f64>().ok(),
        _ => None,
    }
}

/// 宽容解析窗口重置时间（秒）：兼容多种字段名/单位/绝对时间戳
fn resets_in_seconds(w: &Value) -> Option<i64> {
    // 相对秒数：resets_in_seconds / reset_after_seconds / resets_after_seconds
    for k in ["resets_in_seconds", "reset_after_seconds", "resets_after_seconds", "reset_in_seconds"] {
        if let Some(v) = w.get(k).and_then(num) {
            return Some(v.round() as i64);
        }
    }
    // 相对分钟
    for k in ["resets_in_minutes", "reset_after_minutes", "resets_in_minutes_int"] {
        if let Some(v) = w.get(k).and_then(num) {
            return Some((v * 60.0).round() as i64);
        }
    }
    // 绝对时间戳（秒/毫秒）：换算成剩余秒数
    for k in ["reset_at", "resets_at", "reset_timestamp"] {
        if let Some(v) = w.get(k).and_then(num) {
            let now = now_unix() as f64;
            let ts = if v > 1.0e12 { v / 1000.0 } else { v }; // 毫秒时间戳归一
            let d = (ts - now).round() as i64;
            return Some(d.max(0));
        }
    }
    None
}

/// 宽容解析 wham/usage 响应：rate_limits | rate_limit 两种拼写都认；
/// 窗口键 primary_window|primary 与 secondary_window|secondary 都认
/// （codex app-server 的 account/rateLimits/read 用的是不带 _window 后缀的键）
fn parse_wham(v: &Value) -> (WindowUsage, WindowUsage, Option<Value>) {
    let rl = v.get("rate_limits").or_else(|| v.get("rate_limit")).unwrap_or(&Value::Null);
    let win = |keys: &[&str]| {
        let w = keys.iter().find_map(|k| rl.get(k)).unwrap_or(&Value::Null);
        WindowUsage {
            used_percent: w.get("used_percent").and_then(num)
                .or_else(|| w.get("used_percent_human").and_then(num)),
            window_minutes: w.get("window_minutes").and_then(num).map(|m| m.round() as u64),
            resets_in_seconds: resets_in_seconds(w),
        }
    };
    let credits = v.get("credits").cloned()
        .or_else(|| v.get("credits_balance").map(|b| serde_json::json!({ "balance": b })));
    (win(&["primary_window", "primary"]), win(&["secondary_window", "secondary"]), credits)
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

/// 失败原因文本里是否含网络类错误（超时 / 连接失败 / HTTP 5xx）。
/// probe 与 fetch_all 的 attempted 都是“{url}: {err}”或“{url} -> HTTP {code}”形态，
/// 这里只做保守的子串匹配：命中才归为 network（可重试），否则交回 no_login/no_relay。
fn looks_like_network_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "timed out",
        "timeout",
        "connection refused",
        "connection reset",
        "connection closed",
        "error sending request",
        "dns",
        "temporary failure",
        "network",
        "http 500",
        "http 502",
        "http 503",
        "http 504",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
}

/// 数据源失败归因（纯函数，便于单测）：把三级回退的结果收敛为前端可引导的状态。
///
/// 优先级（自上而下，命中即返回）——**配置类判定先于网络启发式**：
///   1. `ok`       —— 成功取到用量；
///   2. `no_relay` —— 有 key 但缺 base_url（中转只配了一半），引导补齐 base_url；
///   3. `no_login` —— **§4 硬保证**：完全没有任何凭据（既无 OAuth access_token、也没有 key），
///                    即便失败原因像网络故障也归此态，首要动作是登录（`codex login`）；
///   4. `network`  —— 只配了中转（有 key、无 OAuth）却遇到网络类失败 → 先按网络"稍后重试"；
///   5. `no_login` —— 无 OAuth 且非网络类失败（如本机 relay 404）→ 仍是"去登录 Codex"；
///   6. `network`  —— 凭据齐备（有 OAuth）后的网络类失败；
///   7. `none`     —— 其余（已配置却全部失败：relay 404、接口不兼容等），通用提示。
///
/// 注：第 3 步（`!has_login && !has_key`）**不是死代码**——它必须先于第 4 步，
/// 否则"完全无凭据 + 网络类失败"会被抢成 `network`，而 §4 要求它归 `no_login`。
///
/// 入参（均为"具备条件"判定，不含任何密钥值）：
///   - `ok`：是否成功；
///   - `has_login`：auth.json 是否提供了 ChatGPT OAuth access_token；
///   - `has_base`：config.toml 是否配置了 base_url；
///   - `has_key`：是否有可用密钥（config.toml bearer / auth.json api_key）；
///   - `network_error`：attempted 里是否含网络类错误。
pub fn classify_status(
    ok: bool,
    has_login: bool,
    has_base: bool,
    has_key: bool,
    network_error: bool,
) -> &'static str {
    if ok {
        return "ok";
    }
    // 有密钥但缺中转地址：只差 base_url → 引导补齐
    if has_key && !has_base {
        return "no_relay";
    }
    // §4 硬保证：完全没有凭据（无 OAuth、无 key）→ 即便像网络故障也算"未登录"
    if !has_login && !has_key {
        return "no_login";
    }
    // 只配了中转（有 key、无 OAuth）遇网络类失败 → 先按网络重试
    if !has_login && network_error {
        return "network";
    }
    // 无 OAuth 且非网络类（如本机 relay 404）→ 仍引导去登录 Codex
    if !has_login {
        return "no_login";
    }
    // 凭据齐备（有 OAuth）后的网络类失败
    if network_error {
        return "network";
    }
    "none"
}

/// 依次尝试全部数据源；同时从 auth.json 补充 plan/email 身份信息
pub fn fetch_all() -> Usage {
    let auth = read_auth();
    let config = read_config();
    let account_id = auth.as_ref().and_then(|a| a.id_token.as_ref())
        .and_then(|t| identity_from_id_token(t).1.clone());

    // 归因所需的“具备条件”判定（不涉及任何密钥值）
    let has_login = auth.as_ref().map_or(false, |a| a.access_token.is_some());
    let has_base = config.as_ref().and_then(|c| c.0.clone()).is_some();
    let has_key = config.as_ref().and_then(|c| c.1.clone())
        .or_else(|| auth.as_ref().and_then(|a| a.api_key.clone()))
        .is_some();

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

    // 失败归因：结果状态收敛为 ok/no_login/no_relay/network/none，供前端三态引导
    let network_error = attempted.iter().any(|e| looks_like_network_error(e));
    usage.status = classify_status(usage.ok, has_login, has_base, has_key, network_error).to_string();

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
    println!("ok={} source={} status={}", u.ok, u.source, u.status);
    println!("plan={:?} email={:?}", u.plan, u.email);
    println!("primary: used={:?} window_min={:?} reset_in={:?}", u.primary.used_percent, u.primary.window_minutes, u.primary.resets_in_seconds);
    println!("secondary: used={:?} window_min={:?} reset_in={:?}", u.secondary.used_percent, u.secondary.window_minutes, u.secondary.resets_in_seconds);
    println!("credits={:?}", u.credits.is_some());
    if let Some(e) = &u.error { println!("error: {}", e); }
}

#[cfg(test)]
mod tests {
    use super::{classify_status, looks_like_network_error};

    // classify_status 参数顺序：ok, has_login, has_base, has_key, network_error

    #[test]
    fn status_ok_wins() {
        assert_eq!(classify_status(true, false, false, false, true), "ok");
        assert_eq!(classify_status(true, true, true, true, false), "ok");
    }

    #[test]
    fn status_no_login_beats_network_heuristic() {
        // 关键回归：完全没有凭据（既没登录、也没 key）时，即使失败原因里含超时等网络类
        // 错误，也必须归为 no_login —— 用户要做的是登录 Codex，而不是查网络/代理。
        assert_eq!(classify_status(false, false, false, false, true), "no_login");
        assert_eq!(classify_status(false, false, false, false, false), "no_login");
        // 有 base_url 但既没登录也没 key → 仍无凭据 → no_login
        assert_eq!(classify_status(false, false, true, false, true), "no_login");
    }

    #[test]
    fn status_no_relay_when_key_without_base() {
        // 有密钥却缺 base_url（config.toml 只配了一半）→ 引导补齐中转地址
        assert_eq!(classify_status(false, false, false, true, false), "no_relay");
        // 即使同时命中网络错误，缺 base_url 仍是首要问题
        assert_eq!(classify_status(false, false, false, true, true), "no_relay");
    }

    #[test]
    fn status_network_only_after_credentials_ok() {
        // 凭据齐备（登录，或有 key+base_url）之后，网络类失败才归 network（可重试）
        assert_eq!(classify_status(false, true, true, false, true), "network");
        assert_eq!(classify_status(false, true, true, true, true), "network");
    }

    #[test]
    fn status_none_when_configured_but_failed() {
        // 配置齐备却全部失败（relay 404 / 接口不兼容等）→ 通用提示
        assert_eq!(classify_status(false, true, true, true, false), "none");
        assert_eq!(classify_status(false, true, true, false, false), "none");
    }

    #[test]
    fn status_section_4_hard_guarantee_and_relay_ordering() {
        // 本机场景：relay 配置齐（有 key + base_url）但无 OAuth、relay 404（非网络类）
        // → 归 no_login，给出"去登录 Codex"这条路（§5.4 称本机为"无 OAuth 登录态"）
        assert_eq!(classify_status(false, false, true, true, false), "no_login");
        // relay-only（有 key、无 OAuth）遇网络类失败 → 先按网络"重试"，而不是去登录
        assert_eq!(classify_status(false, false, true, true, true), "network");
        // 有 OAuth、有 key，但缺 base_url → 中转地址未配 → no_relay
        assert_eq!(classify_status(false, true, false, true, false), "no_relay");
    }

    #[test]
    fn network_error_detection() {
        assert!(looks_like_network_error("https://x/y: error sending request: timed out"));
        assert!(looks_like_network_error("https://x/y -> HTTP 503"));
        assert!(looks_like_network_error("connection refused"));
        assert!(!looks_like_network_error("https://x/y -> HTTP 404"));
        assert!(!looks_like_network_error("rest: auth.json 无 tokens.access_token"));
    }
}
