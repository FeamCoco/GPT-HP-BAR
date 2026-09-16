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

/// 窗口长度归档（见 [`classify_window`]）。**判定只看窗口长度字段，不看槽位名**：
/// Plus 的 5h 落在 `primary_window`、周窗口落在 `secondary_window` 只是惯例 ——
/// Pro 5x/20x 只有周额度时，那个周窗口本身就占着 primary 槽位。
/// Codex 自家前端也是按 `limit_window_seconds / 60` 分桶，不认槽位名。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    /// 短窗口档（≤12h）：Plus 的滚动 5h = 300min 落在这里
    FiveHour,
    /// ≈24h
    Daily,
    /// ≈7d（10080min）
    Weekly,
    /// ≥30d（年窗口也归此档）
    Monthly,
    /// 有时长但不在已知档位
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct WindowUsage {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_in_seconds: Option<i64>,
    /// 窗口长度归档（由 `window_minutes` / `limit_window_seconds` 归一而来）。
    /// 数据源没给窗口长度时为 None —— 前端据此把主行标签在「5h 窗口 / 本周」之间切换。
    #[serde(default)]
    pub kind: Option<WindowKind>,
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
    /// 是否存在 5h（短窗口）额度，三态：
    ///   `Some(true)`  —— 有 5h 窗口（Plus 的正常态）
    ///   `Some(false)` —— 只有周（及以上）额度（Pro 5x/20x、Plus 的临时取消期）
    ///   `None`        —— 判不出来（数据源没给窗口长度，plan 也不认识）
    /// 前端只把 `false` 当作"主行其实是周"的依据，`None` 一律沿用旧假设。
    #[serde(default)]
    pub has_5h: Option<bool>,
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

/// 窗口长度（分钟）。优先 codex app-server / codex-rs 的 `window_minutes`
/// （其 camelCase 拼写 `windowDurationMins` 也认），其次 REST `wham/usage` 原生的
/// `limit_window_seconds`（18000 = 5h / 604800 = 7d）。
///
/// 两条纪律：
///   · **只认"窗口长度"字段** —— 绝不用 `resets_in_seconds` 反推：剩余时间随用量
///     递减，拿它当长度会把 5h 窗口在快重置时判成短窗口、刚重置时判成长窗口；
///   · 不认识就 `None`（宁可标"未知"交给 plan 兜底，也不要猜一个档位出来）。
fn window_minutes(w: &Value) -> Option<u64> {
    for k in ["window_minutes", "windowDurationMins", "window_duration_mins", "limit_window_minutes"] {
        if let Some(v) = w.get(k).and_then(num) {
            if v > 0.0 { return Some(v.round() as u64); }
        }
    }
    for k in ["limit_window_seconds", "window_seconds", "limit_window_secs", "window_secs"] {
        if let Some(v) = w.get(k).and_then(num) {
            if v > 0.0 { return Some((v / 60.0).round() as u64); }
        }
    }
    None
}

/// 由窗口长度（分钟）归档。阈值与 Codex 自家前端一致；**必须降序判定** ——
/// 升序判的话月/年窗口会先被 `>= 10079` 的 Weekly 吃掉。
///
/// 721~1438min（12h~24h）之间刻意留空返回 `Unknown`：现实中不存在这种窗口，
/// 真出现说明字段语义变了，此时标"未知"比硬塞进某一档安全。
fn classify_window(mins: f64) -> WindowKind {
    if !(mins > 0.0) {
        WindowKind::Unknown
    } else if mins >= 43_200.0 {
        WindowKind::Monthly        // ≥30d
    } else if mins >= 10_079.0 {
        WindowKind::Weekly         // ≥7d − 1min
    } else if mins >= 1_439.0 {
        WindowKind::Daily          // ≥24h − 1min
    } else if mins <= 720.0 {
        WindowKind::FiveHour       // ≤12h：短窗口档
    } else {
        WindowKind::Unknown
    }
}

/// 槽位排序权重：越"短"的窗口越靠前（占主行）
fn window_rank(k: Option<WindowKind>) -> u8 {
    match k {
        Some(WindowKind::FiveHour) => 0,
        Some(WindowKind::Daily) => 1,
        Some(WindowKind::Weekly) => 2,
        Some(WindowKind::Monthly) => 3,
        None | Some(WindowKind::Unknown) => 4,
    }
}

/// 判定"是否存在 5h（短窗口）额度"，三态见 [`Usage::has_5h`]。顺序即优先级：
///   1. **窗口时长已知就是权威** —— 按归档结果说话，plan 说什么都不改它。
///      Plus 在 2026-07-12 ~ 08-25 那段临时取消期里也只返回周窗口，只有按时长判才对；
///   2. 时长未知但两个窗口并存（数据源太老 / 中转把字段吃掉了）→ `None`，
///      把"要不要当 5h"交回前端沿用旧假设；
///   3. 只剩一个窗口且时长未知 → 才用 plan 兜底：pro / prolite 判"无 5h"，
///      plus / business / team / edu / enterprise 判"有"，其余一律 `None`（不猜）。
fn infer_has_5h(primary: &WindowUsage, secondary: &WindowUsage, plan: Option<&str>) -> Option<bool> {
    if let Some(k) = primary.kind {
        return Some(k == WindowKind::FiveHour);
    }
    if let Some(k) = secondary.kind {
        return Some(k == WindowKind::FiveHour);
    }
    if secondary.used_percent.is_some() || primary.used_percent.is_none() {
        return None;
    }
    let norm: String = plan.unwrap_or("").trim().to_ascii_lowercase()
        .chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    match norm.as_str() {
        "pro" | "prolite" | "pro5x" | "pro20x" => Some(false),
        "plus" | "business" | "team" | "edu" | "enterprise" => Some(true),
        _ => None,
    }
}

/// 把「按槽位」的 primary/secondary 归一成「按窗口长度」的语义。
///
/// 这是"没有 5h 额度"在数据层的落点：
///   · Plus 正常态        → primary = 5h、secondary = 周（与旧行为一致）
///   · Pro 5x / 20x       → 只有周窗口，它落在哪个槽位都不重要 → primary = 周、
///                          **secondary 置空**（前端把主行标签改成「本周」，
///                          且不再另起一行重复同一个数字）
///   · API-key / 无窗口   → 两个都空；不是"5h 剩 0%"，也就绝不伪造 100% 占位
///   · 槽位颠倒（5h 在后）→ 按长度排回来
///
/// 副行只有"确实是另一个窗口"时才保留；主行空而副行有值时再交换一次，
/// 避免出现「主行 '--%'、副行却有数字」这种自相矛盾的形态。
fn normalize_windows(a: WindowUsage, b: WindowUsage, plan: Option<&str>) -> (WindowUsage, WindowUsage, Option<bool>) {
    let (mut primary, mut secondary) = if window_rank(b.kind) < window_rank(a.kind) { (b, a) } else { (a, b) };
    if secondary.used_percent.is_none() {
        secondary = WindowUsage::default();
    }
    if primary.used_percent.is_none() && secondary.used_percent.is_some() {
        primary = secondary;
        secondary = WindowUsage::default();
    }
    let has_5h = infer_has_5h(&primary, &secondary, plan);
    (primary, secondary, has_5h)
}

/// 一次 wham / app-server 解析的完整结果（窗口已按长度归一）
struct RateLimits {
    primary: WindowUsage,
    secondary: WindowUsage,
    has_5h: Option<bool>,
    credits: Option<Value>,
    plan: Option<String>,
}

/// 宽容解析 wham/usage 响应：rate_limits | rate_limit 两种拼写都认；
/// 窗口键 primary_window|primary 与 secondary_window|secondary 都认
/// （codex app-server 的 account/rateLimits/read 用的是不带 _window 后缀的键）。
/// `plan_hint` 是 app-server 的 account/read 给出的 plan，仅在响应体自身没有
/// `plan_type` 时兜底 —— plan 参与"5h 是否存在"的最后一级判定，见 [`infer_has_5h`]。
fn parse_wham(v: &Value, plan_hint: Option<&str>) -> RateLimits {
    let rl = v.get("rate_limits").or_else(|| v.get("rate_limit")).unwrap_or(&Value::Null);
    let win = |keys: &[&str]| {
        let w = keys.iter().find_map(|k| rl.get(k)).unwrap_or(&Value::Null);
        let mins = window_minutes(w);
        WindowUsage {
            used_percent: w.get("used_percent").and_then(num)
                .or_else(|| w.get("used_percent_human").and_then(num)),
            window_minutes: mins,
            resets_in_seconds: resets_in_seconds(w),
            kind: mins.map(|m| classify_window(m as f64)),
        }
    };
    // wham/usage 自带 plan_type —— 以前只从 id_token 拿 plan，中转/rest 源的徽标因此
    // 一直是 '--'；顺手用它，也让 infer_has_5h 的兜底有依据。
    let plan = v.get("plan_type").and_then(|x| x.as_str())
        .or_else(|| v.get("plan").and_then(|x| x.as_str()))
        .map(String::from)
        .or_else(|| plan_hint.map(String::from));
    let credits = v.get("credits").cloned()
        .or_else(|| v.get("credits_balance").map(|b| serde_json::json!({ "balance": b })));
    let (primary, secondary, has_5h) = normalize_windows(
        win(&["primary_window", "primary"]),
        win(&["secondary_window", "secondary"]),
        plan.as_deref(),
    );
    RateLimits { primary, secondary, has_5h, credits, plan }
}

fn fetch_rest(auth: &Auth, account_id: Option<&str>) -> Result<Usage, String> {
    let token = auth.access_token.as_ref().ok_or("rest: auth.json 无 tokens.access_token（非 ChatGPT OAuth 登录）")?;
    let v = http_get_json(WHAM_URL, token, account_id)?;
    let r = parse_wham(&v, None);
    Ok(Usage {
        ok: true, source: "rest".into(),
        plan: r.plan,
        credits: r.credits,
        has_5h: r.has_5h,
        primary: r.primary, secondary: r.secondary,
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
    let plan = account.as_ref().and_then(|a| a.get("plan")).and_then(|x| x.as_str()).map(String::from);
    let email = account.as_ref().and_then(|a| a.get("email")).and_then(|x| x.as_str()).map(String::from);
    let r = parse_wham(&rate, plan.as_deref());
    Ok(Usage {
        ok: true, source: "app-server".into(), plan: r.plan, email, credits: r.credits,
        has_5h: r.has_5h,
        primary: r.primary, secondary: r.secondary, fetched_at: now_unix(),
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
                let r = parse_wham(&v, None);
                return Ok(Usage {
                    ok: true, source: "relay".into(), plan: r.plan, credits: r.credits,
                    has_5h: r.has_5h,
                    primary: r.primary, secondary: r.secondary, fetched_at: now_unix(),
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
    println!("has_5h={:?}  (true=有 5h 窗口 / false=只有周及以上额度 / None=判不出来)", u.has_5h);
    println!("primary: used={:?} window_min={:?} kind={:?} reset_in={:?}",
        u.primary.used_percent, u.primary.window_minutes, u.primary.kind, u.primary.resets_in_seconds);
    println!("secondary: used={:?} window_min={:?} kind={:?} reset_in={:?}",
        u.secondary.used_percent, u.secondary.window_minutes, u.secondary.kind, u.secondary.resets_in_seconds);
    println!("credits={:?}", u.credits.is_some());
    if let Some(e) = &u.error { println!("error: {}", e); }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_status, classify_window, infer_has_5h, looks_like_network_error,
        normalize_windows, window_minutes, WindowKind, WindowUsage,
    };
    use serde_json::json;

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

    // ---- 窗口长度归档 / "没有 5h 额度" ----------------------------------------
    // 构造一个窗口：used=已用%，mins=窗口长度（缺省 = 数据源没给长度）
    fn w(used: f64, mins: Option<u64>) -> WindowUsage {
        WindowUsage {
            used_percent: Some(used),
            window_minutes: mins,
            resets_in_seconds: Some(3600),
            kind: mins.map(|m| classify_window(m as f64)),
        }
    }
    fn no_win() -> WindowUsage { WindowUsage::default() }

    #[test]
    fn window_kind_buckets_descending() {
        assert_eq!(classify_window(300.0), WindowKind::FiveHour);
        assert_eq!(classify_window(1440.0), WindowKind::Daily);
        assert_eq!(classify_window(10080.0), WindowKind::Weekly);
        assert_eq!(classify_window(43200.0), WindowKind::Monthly);
        // 年窗口必须先被 Monthly 接住 —— 升序判定会被 Weekly 吃掉
        assert_eq!(classify_window(525_600.0), WindowKind::Monthly);
        // 12h~24h 之间刻意留空：宁可"未知"也不硬塞一档
        assert_eq!(classify_window(1000.0), WindowKind::Unknown);
        assert_eq!(classify_window(0.0), WindowKind::Unknown);
        assert_eq!(classify_window(-1.0), WindowKind::Unknown);
    }

    #[test]
    fn window_minutes_reads_both_spellings() {
        // codex app-server（codex-rs 序列化）
        assert_eq!(window_minutes(&json!({"window_minutes": 300})), Some(300));
        assert_eq!(window_minutes(&json!({"windowDurationMins": 10080})), Some(10080));
        // REST wham/usage 原生：limit_window_seconds
        assert_eq!(window_minutes(&json!({"limit_window_seconds": 18000})), Some(300));
        assert_eq!(window_minutes(&json!({"limit_window_seconds": 604800})), Some(10080));
        assert_eq!(window_minutes(&json!({"used_percent": 42})), None);
        // 关键回归：resets_in_seconds 不能当窗口长度用（它随用量递减，
        // 拿它当长度会让 5h 窗口在快重置时"变短"、刚重置时"变长"）
        assert_eq!(window_minutes(&json!({"resets_in_seconds": 120})), None);
        assert_eq!(window_minutes(&json!({"window_minutes": 0})), None);
    }

    #[test]
    fn plus_two_windows_keeps_5h_as_primary() {
        let (p, s, has) = normalize_windows(w(12.0, Some(300)), w(40.0, Some(10080)), Some("plus"));
        assert_eq!(p.kind, Some(WindowKind::FiveHour));
        assert_eq!(s.kind, Some(WindowKind::Weekly));
        assert_eq!(has, Some(true));
    }

    #[test]
    fn pro_weekly_only_promotes_weekly_and_drops_secondary() {
        // Pro 5x / 20x 的真实形态：只有一个窗口、长度 10080min，而且就占着 primary 槽位。
        // 旧实现会把它当"5h 窗口"显示 —— 主行标签写 5h、数字其实是周剩余，副行还是 '--%'。
        let (p, s, has) = normalize_windows(w(63.0, Some(10080)), no_win(), Some("pro"));
        assert_eq!(has, Some(false));
        assert_eq!(p.kind, Some(WindowKind::Weekly));
        assert_eq!(p.used_percent, Some(63.0));
        assert!(s.used_percent.is_none(), "副行必须置空，否则同一个数字会占两行");
    }

    #[test]
    fn five_hour_only_keeps_5h_and_hides_week() {
        let (p, s, has) = normalize_windows(w(12.0, Some(300)), no_win(), Some("plus"));
        assert_eq!(p.kind, Some(WindowKind::FiveHour));
        assert_eq!(has, Some(true));
        assert!(s.used_percent.is_none());
    }

    #[test]
    fn swapped_slots_are_realigned_by_length() {
        // 槽位颠倒（5h 落在 secondary）→ 按长度排回主行，别信槽位名
        let (p, s, has) = normalize_windows(w(40.0, Some(10080)), w(12.0, Some(300)), Some("plus"));
        assert_eq!(p.kind, Some(WindowKind::FiveHour));
        assert_eq!(p.used_percent, Some(12.0));
        assert_eq!(s.kind, Some(WindowKind::Weekly));
        assert_eq!(has, Some(true));
    }

    #[test]
    fn duration_beats_plan() {
        // 时长已知就是权威：plan=plus 但只给了一个周窗口 ——
        // 2026-07-12~08-25 那段"临时取消 5h"的时期就是这个形态，必须判"无 5h"
        assert_eq!(infer_has_5h(&w(40.0, Some(10080)), &no_win(), Some("plus")), Some(false));
        // 反向同理：plan=pro 但给了一个 5h 窗口（将来 Pro 恢复 5h 时）
        assert_eq!(infer_has_5h(&w(12.0, Some(300)), &no_win(), Some("pro")), Some(true));
    }

    #[test]
    fn unknown_duration_falls_back_to_plan_then_gives_up() {
        // 单窗口 + 时长未知 → plan 兜底
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("pro")).2, Some(false));
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("prolite")).2, Some(false));
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("pro_5x")).2, Some(false));
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("Plus")).2, Some(true));
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("business")).2, Some(true));
        // plan 不认识 / 没有 plan → None（不猜，前端沿用旧假设）
        assert_eq!(normalize_windows(w(63.0, None), no_win(), Some("weird")).2, None);
        assert_eq!(normalize_windows(w(63.0, None), no_win(), None).2, None);
        // 两窗口并存 + 时长都未知 → 视为旧布局 → None（不因为 plan=pro 就改判）
        assert_eq!(normalize_windows(w(12.0, None), w(40.0, None), Some("pro")).2, None);
    }

    #[test]
    fn no_windows_at_all_reports_nothing() {
        // API-key / 按量计费模式：没有窗口额度，也不该伪造 100% / 0% 占位
        let (p, s, has) = normalize_windows(no_win(), no_win(), Some("plus"));
        assert!(p.used_percent.is_none() && s.used_percent.is_none());
        assert_eq!(has, None);
        assert!(p.kind.is_none() && s.kind.is_none());
    }

    #[test]
    fn value_on_secondary_only_moves_up_to_primary() {
        // 主行空、副行有值 → 交换，避免「主行 '--%'、副行有数字」的自相矛盾形态
        let (p, s, _) = normalize_windows(no_win(), w(40.0, Some(10080)), Some("pro"));
        assert_eq!(p.used_percent, Some(40.0));
        assert!(s.used_percent.is_none());
    }

    #[test]
    fn daily_plus_weekly_keeps_two_rows() {
        // 罕见但存在：日窗口 + 周窗口 —— 两个都不是 5h，但都该显示
        let (p, s, has) = normalize_windows(w(30.0, Some(1440)), w(70.0, Some(10080)), None);
        assert_eq!(p.kind, Some(WindowKind::Daily));
        assert_eq!(s.kind, Some(WindowKind::Weekly));
        assert_eq!(has, Some(false));
    }
}
