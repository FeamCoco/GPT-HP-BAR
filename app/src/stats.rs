//! GPT-HP-BAR 模型用量统计与当前 token 速度
//!
//! 数据来源：本机 Codex 会话记录 `$CODEX_HOME/sessions/**/*.jsonl`（rollout 文件，
//! Codex CLI 与 ChatGPT 桌面版共用同一账号池，都会写这里）。每个响应完成时落一行：
//!   · `token_usage_record` —— 单响应用量 {input/cached/output/reasoning/total}
//!   · `event_msg` + `token_count` —— `info.last_token_usage` 为单响应增量、
//!     `info.total_token_usage` 为线程累计（老版 CLI 没有前者，字段平铺在 payload 上）
//!   · `turn_context` / `thread_settings_applied` —— 本轮使用的模型名
//! 所以"各模型用量 / 当前速度"不需要任何新远端接口：增量 tail 这些 JSONL 即可，
//! 纯本地、零上传、与额度数据源（wham / app-server / relay）完全解耦 ——
//! 中转账号没有 5h 窗口也能看到自己真实跑了多少 token。
//!
//! 扫描是**增量**的：每个文件记住已读字节偏移，每轮只解析新追加的行；
//! 文件被截断/重写（offset 越界）则整体重读，绝不重复计数。
//! 窗口只看"本地今天 + 近 7 天"，更早的行解析到也不进快照。
//!
//! "当前 token 速度" = 最近活跃会话里，相邻两次响应完成之间的输出 token 增量
//! ÷ 时间差，在滚动 2 分钟窗口内取平均（窗口内不足两个采样点则不给速度，
//! 但"活跃"状态由最近事件时间单独决定）。本模块只输出 tok/s 数字；
//! tok/s -> 载具档位（🐢 单车 🛵 …）的映射在前端 i18n.js 的 speedTier。
//!
//! 安全约定：只读会话记录，绝不输出任何密钥；probe 打印不含 token 值。

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone, Timelike, Utc};

/// 速度采样窗口（毫秒）：取最近 2 分钟内的"响应完成事件"对算平均输出速度。
/// Codex 活跃时每 4~20s 完成一个响应，2 分钟窗口能攒 6~20 个采样点，
/// 既压平单次小响应的抖动，又不至于把几分钟前的旧速度当成"当前"。
const SPEED_WINDOW_MS: i64 = 120_000;
/// 会话"活跃"判定：最近一次 token 事件距今 ≤ 3 分钟（含工具调用长间隙）。
const ACTIVE_SECS: i64 = 180;
/// 统计窗口：本地今天 + 近 7 天（快照只含这 8 天，更早的行不进结果）。
const KEEP_DAYS: i64 = 7;
/// 每文件速度采样环上限（活跃时每分钟 ~10 个点，64 足够覆盖窗口）。
const SAMPLE_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 对外结构（serde 字段 snake_case，与 datasource::Usage 的前端契约一致）
// ---------------------------------------------------------------------------

/// 单模型的用量汇总
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct ModelUsage {
    pub model: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
    /// 响应（API call）条数：turn 数的近似上界，用来感知"跑了多少轮"
    pub responses: u64,
}

/// 一次扫描的完整快照（emit "stats" / get_stats 的载荷）
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct Stats {
    pub ok: bool,
    /// 本地日期 YYYY-MM-DD（today 的口径）
    pub day: String,
    /// 今日各模型用量（按 total 降序）
    pub today: Vec<ModelUsage>,
    /// 近 7 天（含今日）各模型用量
    pub week: Vec<ModelUsage>,
    pub today_total: u64,
    pub week_total: u64,
    /// 今日 24 小时用量分布（本地时，小时 = 总 token 数）
    pub hours: [u64; 24],
    /// 当前输出速度（tok/s）；窗口内采样不足时为 None
    pub speed_tok_s: Option<f64>,
    /// 最近 3 分钟内有 token 事件（生成中，哪怕速度暂时算不出）
    pub active: bool,
    /// 距最近一次 token 事件的秒数
    pub last_event_age_secs: Option<u64>,
    /// 窗口内扫到的会话文件数
    pub files: usize,
    pub codex_home: String,
    pub scanned_at: u64,
}

// ---------------------------------------------------------------------------
// 内部聚合结构
// ---------------------------------------------------------------------------

/// 从 JSON 抽出的单响应用量（宽容提取：缺字段按 0）
#[derive(Default, Clone, Copy)]
struct Raw {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

/// (本地日, 模型) 维度的用量聚合
#[derive(Default, Clone)]
struct Agg {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
    responses: u64,
}

impl Agg {
    fn add(&mut self, r: &Raw) {
        self.input += r.input;
        self.cached += r.cached;
        self.output += r.output;
        self.reasoning += r.reasoning;
        self.total += r.total;
        self.responses += 1;
    }
}

/// 单会话文件的增量解析状态
#[derive(Default, Clone)]
struct FileState {
    /// 已解析到的字节偏移（永远停在 '\n' 边界）
    offset: u64,
    /// 上次看到的 mtime（未变化直接跳过重读）
    mtime: i64,
    /// turn_id -> 模型名（turn_context 写入；token_usage_record 按 turn 归属）
    turn_models: HashMap<String, String>,
    /// 文件级"当前模型"兜底（thread_settings_applied / 最新 turn_context）
    file_model: Option<String>,
    /// token_usage_record 聚合（新版 CLI，权威来源）
    agg_records: HashMap<(String, String), Agg>,
    /// token_count 聚合（老版 CLI 的回退来源；两类并存时 records 优先）
    agg_tc: HashMap<(String, String), Agg>,
    /// (本地日, 本地时) -> 总 token（画今日小时分布）
    hours: HashMap<(String, u8), u64>,
    /// 文件内 token_usage_record 条数（>0 说明该文件用 records 口径）
    records_count: u64,
    /// response_id 去重（同文件内同一响应只计一次）
    resp_seen: HashSet<String>,
    /// 速度采样环：(事件时刻 ms, 累计输出 token)
    samples: VecDeque<(i64, u64)>,
    /// 最近一次 token 事件时刻 ms（活跃判定 + 多会话选主）
    last_event_ms: Option<i64>,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn codex_home() -> PathBuf {
    if let Ok(p) = std::env::var("CODEX_HOME") {
        return PathBuf::from(p);
    }
    let mut p = std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    p.push(".codex");
    p
}

fn sessions_dir() -> PathBuf {
    codex_home().join("sessions")
}

/// RFC3339 -> (本地日, 本地时)。时间戳一律 UTC（codex 写 "….Z"），
/// "今天"的口径必须落到**本地时区**，否则早上 8 点前会把昨晚算进"今天"。
fn local_day_hour(ts: &str) -> Option<(String, u8)> {
    let dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(ts).ok()?;
    let l = dt.with_timezone(&Local);
    Some((l.date_naive().to_string(), l.hour() as u8))
}

/// RFC3339 -> UTC 毫秒时间戳
fn ts_ms(ts: &str) -> Option<i64> {
    let dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(ts).ok()?;
    Some(dt.with_timezone(&Utc).timestamp_millis())
}

fn u64_of(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0)
}

fn raw_from(v: &Value) -> Raw {
    Raw {
        input: u64_of(v.get("input_tokens")),
        cached: u64_of(v.get("cached_input_tokens")),
        output: u64_of(v.get("output_tokens")),
        reasoning: u64_of(v.get("reasoning_output_tokens")),
        total: u64_of(v.get("total_tokens")),
    }
}

/// 从 event_msg/token_count payload 抽单响应用量：
/// 新版在 info.last_token_usage；老版平铺在 payload 上
fn token_count_usage(payload: &Value) -> Option<Raw> {
    if let Some(info) = payload.get("info") {
        if let Some(last) = info.get("last_token_usage") {
            return Some(raw_from(last));
        }
    }
    if payload.get("output_tokens").is_some() {
        return Some(raw_from(payload));
    }
    None
}

/// token_count 的累计输出（算速度用）：优先 info.total_token_usage（绝对累计）；
/// 拿不到（老版平铺字段 / 本响应输出为 0）就"上个采样点累计 + 本响应输出"续接 ——
/// 两种模式在单个文件里不会混（一个进程写一个文件），续接保证采样序列单调、
/// 不会插进一个假 0 把速度差分撑爆。
fn token_count_cum_out(payload: &Value, prev_cum: u64, r: &Raw) -> u64 {
    if let Some(t) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
        let c = u64_of(t.get("output_tokens"));
        if c > 0 {
            return c;
        }
    }
    prev_cum + r.output
}

/// 当前模型归属：turn_id 精确匹配 -> 文件级兜底 -> unknown。
/// 归属不到模型的用量也要入账（宁可显示"未知模型"也不丢数字）。
fn attribute<'a>(st: &'a FileState, turn_id: Option<&str>) -> String {
    if let Some(t) = turn_id {
        if let Some(m) = st.turn_models.get(t) {
            return m.clone();
        }
    }
    st.file_model.clone().unwrap_or_else(|| "unknown".into())
}

/// 单行解析（纯函数，测试直达）：把一条 rollout 记录并进 FileState。
/// 模型归属与聚合口径见文件头注释；解析失败的行静默跳过（宽容映射纪律）。
fn apply_line(st: &mut FileState, line: &str) {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let ts = v.get("timestamp").and_then(|x| x.as_str()).unwrap_or("");
    let typ = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let payload = v.get("payload").cloned().unwrap_or(Value::Null);

    match typ {
        "turn_context" => {
            let model = payload.get("model").and_then(|x| x.as_str()).map(String::from);
            if let Some(m) = model.clone() {
                st.file_model = Some(m);
            }
            if let (Some(tid), Some(m)) = (
                payload.get("turn_id").and_then(|x| x.as_str()).map(String::from),
                model,
            ) {
                st.turn_models.insert(tid, m);
            }
        }
        "token_usage_record" => {
            let Some(usage) = payload.get("usage").map(raw_from) else { return };
            let rid = payload.get("response_id").and_then(|x| x.as_str()).map(String::from);
            if let Some(id) = &rid {
                if !st.resp_seen.insert(id.clone()) {
                    return; // 同一响应重复落盘（断点续写等），只计一次
                }
            }
            let turn = payload.get("turn_id").and_then(|x| x.as_str());
            let model = attribute(st, turn);
            if let Some((day, _hour)) = local_day_hour(ts) {
                st.agg_records.entry((day, model)).or_default().add(&usage);
            }
            st.records_count += 1;
        }
        "event_msg" => match payload.get("type").and_then(|x| x.as_str()).unwrap_or("") {
            "thread_settings_applied" => {
                if let Some(m) = payload
                    .get("thread_settings")
                    .and_then(|s| s.get("model"))
                    .and_then(|x| x.as_str())
                {
                    st.file_model = Some(m.to_string());
                }
            }
            "token_count" => {
                let Some(r) = token_count_usage(&payload) else { return };
                let Some((day, hour)) = local_day_hour(ts) else { return };
                let prev = st.samples.back().map(|(_, c)| *c).unwrap_or(0);
                let cum = token_count_cum_out(&payload, prev, &r);
                let model = attribute(st, None);
                st.agg_tc.entry((day.clone(), model)).or_default().add(&r);
                *st.hours.entry((day, hour)).or_default() += r.total;
                let now_ms = ts_ms(ts).unwrap_or(0);
                if now_ms > 0 {
                    st.samples.push_back((now_ms, cum));
                    st.last_event_ms = Some(now_ms);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// 速度：取窗口内首尾采样点 (Δ累计输出 / Δt)。窗口内不足两点给 None ——
/// 响应间隔可能被工具调用拉长，硬算会得到误导性的"接近 0"。
fn compute_speed(samples: &VecDeque<(i64, u64)>, now_ms: i64) -> Option<f64> {
    let cut = now_ms - SPEED_WINDOW_MS;
    let pts: Vec<&(i64, u64)> = samples.iter().filter(|(t, _)| *t >= cut).collect();
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return None };
    if last.0 <= first.0 {
        return None;
    }
    let dt = (last.0 - first.0) as f64 / 1000.0;
    if dt < 2.0 {
        return None;
    }
    Some((last.1.saturating_sub(first.1)) as f64 / dt)
}

/// 把全部 FileState 归并成快照（纯函数）：单文件"records 优先"口径、
/// 今日/近 7 天双窗口、今日小时分布、速度与活跃判定都在这里收敛。
fn snapshot_from(states: &HashMap<PathBuf, FileState>, now_local: DateTime<Local>) -> Stats {
    let today = now_local.date_naive();
    let today_s = today.to_string();
    let week_start = today - chrono::Duration::days(KEEP_DAYS - 1);
    let week_start_s = week_start.to_string();
    let now_ms = now_local.timestamp_millis();

    let mut today_map: HashMap<String, Agg> = HashMap::new();
    let mut week_map: HashMap<String, Agg> = HashMap::new();
    let mut hours = [0u64; 24];
    let mut last_event_ms: Option<i64> = None;
    let mut speed_src: Option<&VecDeque<(i64, u64)>> = None;
    let mut files = 0usize;

    for st in states.values() {
        files += 1;
        // 选"最近还活着"的文件算速度：并发多会话时取最近事件的那个
        if let Some(t) = st.last_event_ms {
            let take = match last_event_ms {
                None => true,
                Some(cur) => t > cur,
            };
            if take {
                last_event_ms = Some(t);
                speed_src = Some(&st.samples);
            }
        }
        // 单文件口径二选一：有 token_usage_record 就用 records（新版 CLI 两个都写，
        // 同一批用量会以 token_count 重复出现，混用会翻倍）；老版只有 token_count
        let source = if st.records_count > 0 { &st.agg_records } else { &st.agg_tc };
        for ((day, model), agg) in source {
            // ISO 日期串的字典序 == 时间序，字符串比较即可
            if *day == today_s {
                let e = today_map.entry(model.clone()).or_default();
                e.input += agg.input;
                e.cached += agg.cached;
                e.output += agg.output;
                e.reasoning += agg.reasoning;
                e.total += agg.total;
                e.responses += agg.responses;
            }
            if day.as_str() >= week_start_s.as_str() && day.as_str() <= today_s.as_str() {
                let e = week_map.entry(model.clone()).or_default();
                e.input += agg.input;
                e.cached += agg.cached;
                e.output += agg.output;
                e.reasoning += agg.reasoning;
                e.total += agg.total;
                e.responses += agg.responses;
            }
        }
        for ((day, hour), v) in &st.hours {
            if *day == today_s {
                hours[*hour as usize] += v;
            }
        }
    }

    let to_list = |m: HashMap<String, Agg>| -> Vec<ModelUsage> {
        let mut v: Vec<ModelUsage> = m
            .into_iter()
            .map(|(model, a)| ModelUsage {
                model,
                input_tokens: a.input,
                cached_input_tokens: a.cached,
                output_tokens: a.output,
                reasoning_output_tokens: a.reasoning,
                total_tokens: a.total,
                responses: a.responses,
            })
            .collect();
        v.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens).then(a.model.cmp(&b.model)));
        v
    };
    let today = to_list(today_map);
    let week = to_list(week_map);
    let today_total = today.iter().map(|m| m.total_tokens).sum();
    let week_total = week.iter().map(|m| m.total_tokens).sum();

    let speed_tok_s = speed_src.and_then(|s| compute_speed(s, now_ms));
    let last_event_age_secs = last_event_ms.map(|t| ((now_ms - t).max(0) / 1000) as u64);
    let active = matches!(last_event_age_secs, Some(age) if age as i64 <= ACTIVE_SECS);

    Stats {
        ok: week_total > 0,
        day: today_s,
        today,
        week,
        today_total,
        week_total,
        hours,
        speed_tok_s,
        active,
        last_event_age_secs,
        files,
        codex_home: sessions_dir().to_string_lossy().into_owned(),
        scanned_at: now_unix(),
    }
}

/// 递归收集 `sessions/YYYY/MM/DD/*.jsonl`。入选规则（满足其一）：
///   · 日期目录在窗口内 —— 里面的行只可能是新写的；
///   · 文件 mtime 在窗口内 —— **老目录的长会话仍在追加**（跨多天 / resume）时
///     目录日期早已出窗，但新增的事件就在最近，必须跟着 mtime 收进来
///     （实机验证：2026-11-01 的 gpt-6.1-sol 会话就是这么漏掉的，修复后对上）。
/// 其余忽略；未来日期的目录也收（容忍时钟偏差）。
fn collect_jsonl(root: &std::path::Path, window_start: NaiveDate, cutoff_epoch: i64, out: &mut Vec<(PathBuf, i64, u64)>) {
    let Ok(yrs) = std::fs::read_dir(root) else { return };
    for y in yrs.flatten() {
        let Some(yv) = y.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let Ok(mos) = std::fs::read_dir(y.path()) else { continue };
        for mo in mos.flatten() {
            let Some(mv) = mo.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            let Ok(days) = std::fs::read_dir(mo.path()) else { continue };
            for d in days.flatten() {
                let Some(dv) = d.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
                let Some(date) = NaiveDate::from_ymd_opt(yv, mv, dv) else { continue };
                let dir_in_window = date >= window_start;
                let Ok(fs_) = std::fs::read_dir(d.path()) else { continue };
                for f in fs_.flatten() {
                    let p = f.path();
                    if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let Ok(meta) = f.metadata() else { continue };
                    let mt = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    if !dir_in_window && mt < cutoff_epoch {
                        continue; // 目录出窗且文件也不再更新：里面的行只可能更老
                    }
                    out.push((p, mt, meta.len()));
                }
            }
        }
    }
}

/// 从 offset 读到文件尾（UTF-8 lossy；调用方负责只消费完整行）
fn read_from(path: &std::path::Path, offset: u64) -> Option<String> {
    let mut f = File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 增量扫描器：持有每文件状态，`scan()` 每轮只解析新追加的行
pub struct Scanner {
    state: HashMap<PathBuf, FileState>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Self { state: HashMap::new() }
    }

    /// 全量窗口扫描 -> 快照。开销：未变化的文件只做一次 stat 比对；
    /// 有追加时只读新增字节。可长期每 2s 跑一次。
    pub fn scan(&mut self) -> Stats {
        let now_local = Local::now();
        let today = now_local.date_naive();
        let window_start = today - chrono::Duration::days(KEEP_DAYS - 1);
        // 状态修剪线比快照窗口早一天：跨午夜的长会话昨天已入账，今天还得能读到增量。
        // collect 用同一条线：老目录里仍在追加的文件靠 mtime 入选（见函数注释）
        let prune_start = window_start - chrono::Duration::days(1);
        let prune_epoch = local_midnight_epoch(prune_start);

        let mut files: Vec<(PathBuf, i64, u64)> = Vec::new();
        collect_jsonl(&sessions_dir(), prune_start, prune_epoch, &mut files);
        files.sort();

        for (path, mtime, size) in files {
            // entry 需要所有权；path 还要用于读取，克隆一份进 map（每文件一次，开销可忽略）
            let st = self.state.entry(path.clone()).or_default();
            if st.mtime == mtime && st.offset == size {
                continue; // fast path：文件没动
            }
            if size < st.offset {
                *st = FileState::default(); // 被截断/重写：整体重读，绝不重复计数
            }
            if let Some(buf) = read_from(&path, st.offset) {
                // 只消费到最后一个完整行：半行留给下一轮（写进程可能还在写）
                let consumed = match buf.rfind('\n') {
                    Some(i) => i + 1,
                    None => 0,
                };
                if consumed > 0 {
                    for line in buf[..consumed].lines() {
                        apply_line(st, line);
                    }
                    st.offset += consumed as u64;
                }
                while st.samples.len() > SAMPLE_CAP {
                    st.samples.pop_front();
                }
                st.mtime = mtime;
            }
        }

        // 修剪窗口外文件（连同其聚合与采样一起释放）
        self.state.retain(|_, st| st.mtime >= prune_epoch);

        snapshot_from(&self.state, now_local)
    }
}

/// 本地某天 00:00 的 Unix 秒（夏令时歧义取最早解）
fn local_midnight_epoch(d: NaiveDate) -> i64 {
    let ndt = d.and_hms_opt(0, 0, 0).unwrap_or_else(|| d.and_hms_opt(12, 0, 0).unwrap());
    Local
        .from_local_datetime(&ndt)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 进程内共享快照 + 探针
// ---------------------------------------------------------------------------

/// 最近一次快照（stats 线程写，get_stats 命令读）
pub static LAST: std::sync::Mutex<Option<Stats>> = std::sync::Mutex::new(None);

pub fn store(s: &Stats) {
    if let Ok(mut g) = LAST.lock() {
        *g = Some(s.clone());
    }
}

pub fn snapshot() -> Option<Stats> {
    LAST.lock().ok().and_then(|g| g.clone())
}

/// `--probe-stats`：打印用量统计探测结果（纯本地会话记录，无密钥输出）
pub fn probe() {
    println!("GPT-HP-BAR 用量统计探测（本地 Codex 会话记录）");
    println!("sessions 目录 = {}", sessions_dir().display());
    let mut sc = Scanner::new();
    let s = sc.scan();
    println!("窗口内会话文件 = {}   本地日期 = {}", s.files, s.day);
    println!("活跃={} 最近事件={}s 前 速度={:?}", s.active, s.last_event_age_secs.unwrap_or(0), s.speed_tok_s);
    println!("今日合计 = {} tok   近7天合计 = {} tok", s.today_total, s.week_total);
    let row = |m: &ModelUsage| {
        println!(
            "  {:<24} 输入={:<9} (缓存 {:<9}) 输出={:<9} (推理 {:<9}) 合计={:<9} 响应={}",
            m.model, m.input_tokens, m.cached_input_tokens, m.output_tokens, m.reasoning_output_tokens, m.total_tokens, m.responses
        );
    };
    println!("-- 今日各模型 --");
    if s.today.is_empty() {
        println!("  （无）");
    }
    for m in &s.today {
        row(m);
    }
    println!("-- 近 7 天各模型 --");
    if s.week.is_empty() {
        println!("  （无）");
    }
    for m in &s.week {
        row(m);
    }
    print!("-- 今日小时分布 --\n  ");
    for (h, v) in s.hours.iter().enumerate() {
        print!("{:02}h:{:<8}", h, v);
        if h % 6 == 5 {
            println!();
            print!("  ");
        }
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn st() -> FileState {
        FileState::default()
    }

    fn line(v: serde_json::Value) -> String {
        v.to_string()
    }

    fn ts_at(base: &str, add_ms: i64) -> String {
        let dt = DateTime::parse_from_rfc3339(base).unwrap() + Duration::milliseconds(add_ms);
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    const T0: &str = "2026-11-04T02:00:00.000Z"; // 本地（UTC+8）10:00，当天上午

    /// T0 的本地日期 + 该日正午（"now"）。从 T0 推导而不是硬编码日期，
    /// 测试在任何时区都自洽（断言"今天"的口径随 T0 走）。
    fn t0_today() -> (NaiveDate, DateTime<Local>) {
        let t0 = DateTime::parse_from_rfc3339(T0).unwrap().with_timezone(&Local);
        let d = t0.date_naive();
        let now = Local
            .from_local_datetime(&d.and_hms_opt(12, 0, 0).unwrap())
            .single()
            .unwrap();
        (d, now)
    }

    #[test]
    fn token_count_without_records_aggregates_last_usage() {
        let mut s = st();
        s.file_model = Some("gpt-6-sol".into());
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "input_tokens": 1000, "cached_input_tokens": 400,
                            "output_tokens": 120, "reasoning_output_tokens": 80, "total_tokens": 1120 },
                          "total_token_usage": { "input_tokens": 1000, "output_tokens": 120, "total_tokens": 1120 } } }
        })));
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": ts_at(T0, 8000), "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "input_tokens": 900, "cached_input_tokens": 0,
                            "output_tokens": 300, "reasoning_output_tokens": 200, "total_tokens": 1200 },
                          "total_token_usage": { "input_tokens": 1900, "output_tokens": 420, "total_tokens": 2320 } } }
        })));
        assert_eq!(s.records_count, 0);
        let agg = s.agg_tc.get(&("2026-11-04".into(), "gpt-6-sol".into())).unwrap();
        assert_eq!(agg.output, 420);
        assert_eq!(agg.total, 2320);
        assert_eq!(agg.responses, 2);
        assert_eq!(s.samples.back().unwrap(), &(ts_at(T0, 8000).parse::<DateTime<FixedOffset>>().unwrap().with_timezone(&Utc).timestamp_millis(), 420));
    }

    #[test]
    fn legacy_flat_token_count_is_supported() {
        let mut s = st();
        s.file_model = Some("gpt-6-astra".into());
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "event_msg",
            "payload": { "type": "token_count",
                "input_tokens": 500, "cached_input_tokens": 0, "output_tokens": 64,
                "reasoning_output_tokens": 0, "total_tokens": 564 }
        })));
        let agg = s.agg_tc.values().next().unwrap();
        assert_eq!(agg.total, 564);
        assert_eq!(agg.output, 64);
        assert_eq!(s.samples.back().unwrap().1, 64, "老版无 total 累计：运行累计作为速度样本");
    }

    #[test]
    fn records_take_precedence_and_dedup_by_response_id() {
        let mut s = st();
        let rec = |rid: &str, out: u64| {
            line(serde_json::json!({
                "timestamp": T0, "type": "token_usage_record",
                "payload": { "turn_id": "t1", "response_id": rid,
                    "usage": { "input_tokens": 10, "cached_input_tokens": 0, "output_tokens": out,
                               "reasoning_output_tokens": 0, "total_tokens": 10 + out } }
            }))
        };
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "turn_context",
            "payload": { "turn_id": "t1", "model": "gpt-6.1-terra" } })));
        apply_line(&mut s, &rec("r1", 100));
        apply_line(&mut s, &rec("r1", 100)); // 同响应重复落盘
        apply_line(&mut s, &rec("r2", 30));
        // 同文件也有 token_count（新版两个都写）—— snapshot 时 records 必须赢
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "input_tokens": 10, "output_tokens": 999, "total_tokens": 1009 },
                          "total_token_usage": { "output_tokens": 999, "total_tokens": 1009 } } } })));

        let (today, now) = t0_today();
        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        let terra = snap.today.iter().find(|m| m.model == "gpt-6.1-terra").unwrap();
        assert_eq!(terra.output_tokens, 130, "按 records 口径，token_count 的 999 不许混入");
        assert_eq!(terra.responses, 2);
        let _ = today;
    }

    #[test]
    fn mid_file_model_switch_attributes_per_turn() {
        let mut s = st();
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "turn_context", "payload": { "turn_id": "t1", "model": "gpt-6-sol" } })));
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": ts_at(T0, 1000), "type": "token_usage_record",
            "payload": { "turn_id": "t1", "response_id": "r1",
                "usage": { "output_tokens": 10, "total_tokens": 10 } } })));
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": ts_at(T0, 2000), "type": "turn_context", "payload": { "turn_id": "t2", "model": "codex-auto-review" } })));
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": ts_at(T0, 3000), "type": "token_usage_record",
            "payload": { "turn_id": "t2", "response_id": "r2",
                "usage": { "output_tokens": 20, "total_tokens": 20 } } })));

        let (_today, now) = t0_today();
        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        assert_eq!(snap.today.iter().find(|m| m.model == "gpt-6-sol").unwrap().total_tokens, 10);
        assert_eq!(snap.today.iter().find(|m| m.model == "codex-auto-review").unwrap().total_tokens, 20);
    }

    #[test]
    fn unattributed_usage_lands_on_unknown() {
        let mut s = st();
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "token_usage_record",
            "payload": { "response_id": "r1", "usage": { "output_tokens": 7, "total_tokens": 7 } } })));
        let (_today, now) = t0_today();
        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        assert_eq!(snap.today.iter().find(|m| m.model == "unknown").unwrap().total_tokens, 7);
    }

    #[test]
    fn today_and_week_windows_are_independent() {
        let mut s = st();
        s.file_model = Some("gpt-6-sol".into());
        // 昨天（T0 前 24h，本地日必然是前一天）与今天各一条 token_count
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": ts_at(T0, -86_400_000), "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "output_tokens": 500, "total_tokens": 500 },
                          "total_token_usage": { "output_tokens": 500, "total_tokens": 500 } } } })));
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": T0, "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "output_tokens": 100, "total_tokens": 100 },
                          "total_token_usage": { "output_tokens": 600, "total_tokens": 600 } } } })));
        let (today, now) = t0_today();
        // 今天事件的本地小时（随机器时区推导，不硬编码 +8）
        let h_today = DateTime::parse_from_rfc3339(T0).unwrap().with_timezone(&Local).hour() as usize;

        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        assert_eq!(snap.today_total, 100);
        assert_eq!(snap.week_total, 600);
        assert_eq!(snap.hours[h_today], 100, "今天的事件进今日小时分布");
        assert_eq!(snap.hours.iter().sum::<u64>(), 100, "昨日 500 不得混入今日分布");
        assert_eq!(snap.day, today.to_string());
    }

    #[test]
    fn week_excludes_days_older_than_seven() {
        let mut s = st();
        s.file_model = Some("gpt-6-sol".into());
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": "2026-10-20T10:00:00.000Z", "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "output_tokens": 42, "total_tokens": 42 },
                          "total_token_usage": { "output_tokens": 42, "total_tokens": 42 } } } })));
        let (_today, now) = t0_today();
        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        assert_eq!(snap.week_total, 0);
        assert!(!snap.ok);
    }

    #[test]
    fn speed_uses_recent_window_and_gives_up_without_two_points() {
        let mut q = VecDeque::new();
        let now: i64 = 1_800_000_000_000;
        q.push_back((now - 30_000, 1000));
        q.push_back((now - 10_000, 1600));
        assert_eq!(compute_speed(&q, now), Some(600.0 / 20.0));
        // 窗口外旧点不参与
        let mut q2 = VecDeque::new();
        q2.push_back((now - SPEED_WINDOW_MS - 60_000, 0));
        q2.push_back((now - 10_000, 500));
        assert_eq!(compute_speed(&q2, now), None);
        // 单点给不出速度
        let q3: VecDeque<(i64, u64)> = VecDeque::from(vec![(now - 1000, 42)]);
        assert_eq!(compute_speed(&q3, now), None);
    }

    #[test]
    fn partial_last_line_is_not_consumed() {
        // 模拟"读到半行"：rfind('\n') 只吃到完整行边界（{"a":1} 是 7 字符 + 换行 = 8）
        let buf = "{\"a\":1}\n{\"b\":";
        let consumed = match buf.rfind('\n') {
            Some(i) => i + 1,
            None => 0,
        };
        assert_eq!(consumed, 8);
        assert_eq!(&buf[..consumed], "{\"a\":1}\n");
    }

    #[test]
    fn active_and_age_follow_last_event() {
        let (_today, now) = t0_today();
        let now_ms = now.timestamp_millis();
        let mut s = st();
        s.file_model = Some("gpt-6-sol".into());
        apply_line(&mut s, &line(serde_json::json!({
            "timestamp": Utc.timestamp_millis_opt(now_ms - 60_000).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "type": "event_msg",
            "payload": { "type": "token_count",
                "info": { "last_token_usage": { "output_tokens": 10, "total_tokens": 10 },
                          "total_token_usage": { "output_tokens": 10, "total_tokens": 10 } } } })));
        let mut states = HashMap::new();
        states.insert(PathBuf::from("a.jsonl"), s);
        let snap = snapshot_from(&states, now);
        assert!(snap.active);
        assert_eq!(snap.last_event_age_secs, Some(60));
        assert!(snap.speed_tok_s.is_none(), "窗口内只有单点：活跃但给不出速度");
    }
}
