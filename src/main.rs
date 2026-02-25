// ocrecovery – recovery for the 21st century
// made with <3 by prodbyeternal @ ThinkDifferentInc.
// huge thank you to acidanthera for leading the way of vanilla hackintoshing! :D
//
// rust port woooohoooo

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use num_bigint::BigUint;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, List, ListItem, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

// macrecovery constants

const MLB_ZERO: &str = "00000000000000000";
const TYPE_SID: usize = 16;
const TYPE_K: usize = 64;
const TYPE_FG: usize = 64;

const INFO_IMAGE_LINK: &str = "AU";
const INFO_IMAGE_SESS: &str = "AT";
const INFO_SIGN_LINK: &str = "CU";
const INFO_SIGN_SESS: &str = "CT";

const REQUIRED_KEYS: &[&str] = &["AP", "AU", "AH", "AT", "CU", "CH", "CT"];

const OUT_DIR: &str = "com.apple.recovery.boot";

/// public apple efi rom rsa-2048 public key
const APPLE_EFI_ROM_KEY_HEX: &str =
    "C3E748CAD9CD384329E10E25A91E43E1A762FF529ADE578C935BDDF9B13F2179\
     D4855E6FC89E9E29CA12517D17DFA1EDCE0BEBF0EA7B461FFE61D94E2BDF72C1\
     96F89ACD3536B644064014DAE25A15DB6BB0852ECBD120916318D1CCDEA3C84C9\
     2ED743FC176D0BACA920D3FCF3158AFF731F88CE0623182A8ED67E650515F7574\
     5909F07D415F55FC15A35654D118C55A462D37A3ACDA08612F3F3F6571761EFCC\
     BCC299AEE99B3A4FD6212CCFFF5EF37A2C334E871191F7E1C31960E010A54E86F\
     A3F62E6D6905E1CD57732410A3EB0C6B4DEFDABE9F59BF1618758C751CD56CEF8\
     51D1C0EAA1C558E37AC108DA9089863D20E2E7E4BF475EC66FE6B3EFDCF";

// macOS version definitions


#[derive(Clone)]
struct MacOSVersion {
    name:    &'static str,
    build:   &'static str,
    model:   &'static str,
    os_type: &'static str, // "default" | "latest"
}

fn versions() -> Vec<MacOSVersion> {
    vec![
        MacOSVersion { name: "Lion",          build: "Mac-2E6FAB96566FE58C", model: "00000000000F25Y00", os_type: "default" },
        MacOSVersion { name: "Mountain Lion", build: "Mac-7DF2A3B5E5D671ED", model: "00000000000F65100", os_type: "default" },
        MacOSVersion { name: "Mavericks",     build: "Mac-F60DEB81FF30ACF6", model: "00000000000FNN100", os_type: "default" },
        MacOSVersion { name: "Yosemite",      build: "Mac-E43C1C25D4880AD6", model: "00000000000GDVW00", os_type: "default" },
        MacOSVersion { name: "El Capitan",    build: "Mac-FFE5EF870D7BA81A", model: "00000000000GQRX00", os_type: "default" },
        MacOSVersion { name: "Sierra",        build: "Mac-77F17D7DA9285301", model: "00000000000J0DX00", os_type: "default" },
        MacOSVersion { name: "High Sierra",   build: "Mac-7BA5B2D9E42DDD94", model: "00000000000J80300", os_type: "default" },
        MacOSVersion { name: "Mojave",        build: "Mac-7BA5B2DFE22DDD8C", model: "00000000000KXPG00", os_type: "default" },
        MacOSVersion { name: "Catalina",      build: "Mac-CFF7D910A743CAAF", model: "00000000000PHCD00", os_type: "default" },
        MacOSVersion { name: "Big Sur",       build: "Mac-2BD1B31983FE1663", model: MLB_ZERO,            os_type: "default" },
        MacOSVersion { name: "Monterey",      build: "Mac-E43C1C25D4880AD6", model: MLB_ZERO,            os_type: "default" },
        MacOSVersion { name: "Ventura",       build: "Mac-B4831CEBD52A0C4C", model: MLB_ZERO,            os_type: "default" },
        MacOSVersion { name: "Sonoma",        build: "Mac-827FAC58A8FDFA22", model: MLB_ZERO,            os_type: "default" },
        MacOSVersion { name: "Sequoia",       build: "Mac-7BA5B2D9E42DDD94", model: MLB_ZERO,            os_type: "default" },
        MacOSVersion { name: "Tahoe",         build: "Mac-CFF7D910A743CAAF", model: MLB_ZERO,            os_type: "latest"  },
    ]
}

// shared download-progress state

#[derive(Default, Clone)]
struct DownloadProgress {
    phase: String,

    cnk_downloaded: u64,
    cnk_total:      u64,   // 0 = unknown

    dmg_downloaded: u64,
    dmg_total:      u64,   // 0 = unknown

    verify_chunk: usize,
    verify_total: usize,

    finished: bool,
    error:    Option<String>,

    log: Vec<String>,
}

type SharedProgress = Arc<Mutex<DownloadProgress>>;


// pseudo random id gen

fn generate_id(len: usize) -> String {
    use std::time::SystemTime;
    let hex: &[u8] = b"0123456789ABCDEF";
    let mut seed = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    // xorshift64 + LCG mix for enough spread
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    (0..len)
        .map(|i| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(i as u64 | 1);
            hex[((seed >> 33) ^ (seed >> 17)) as usize % 16] as char
        })
        .collect()
}


// internetrecovery spoof - headers and cookies

type AnyError = Box<dyn std::error::Error + Send + Sync>;

fn get_session(client: &reqwest::blocking::Client) -> Result<String, AnyError> {
    let resp = client
        .get("http://osrecovery.apple.com/")
        .header("Host", "osrecovery.apple.com")
        .header("Connection", "close")
        .header("User-Agent", "InternetRecovery/1.0")
        .send()?;

    for (name, value) in resp.headers() {
        if name.as_str().eq_ignore_ascii_case("set-cookie") {
            for part in value.to_str()?.split("; ") {
                if part.starts_with("session=") {
                    return Ok(part.to_string());
                }
            }
        }
    }
    Err("No session cookie in response headers".into())
}

fn get_image_info(
    client:  &reqwest::blocking::Client,
    session: &str,
    bid:     &str,
    mlb:     &str,
    os_type: &str,
) -> Result<std::collections::HashMap<String, String>, AnyError> {
    let cid = generate_id(TYPE_SID);
    let k   = generate_id(TYPE_K);
    let fg  = generate_id(TYPE_FG);

    let body = format!(
        "cid={}\nsn={}\nbid={}\nk={}\nfg={}\nos={}",
        cid, mlb, bid, k, fg, os_type
    );

    let resp = client
        .post("http://osrecovery.apple.com/InstallationPayload/RecoveryImage")
        .header("Host", "osrecovery.apple.com")
        .header("Connection", "close")
        .header("User-Agent", "InternetRecovery/1.0")
        .header("Cookie", session)
        .header("Content-Type", "text/plain")
        .body(body)
        .send()?;

    let text = resp.text()?;
    let mut info = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(": ") {
            info.insert(k.to_string(), v.to_string());
        }
    }

    for key in REQUIRED_KEYS {
        if !info.contains_key(*key) {
            return Err(format!("Missing required key: {key}").into());
        }
    }
    Ok(info)
}

/// extract hostname from a plain http/s url string
fn url_hostname(url: &str) -> &str {
    let s = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    s.split('/').next().unwrap_or("")
}

// extract last path component from a URL.
fn url_filename(url: &str) -> &str {
    url.split('/').last().unwrap_or("download")
}

/// stream-download `url` to `out_dir` calling `on_progress(downloaded, total_or_0)` each chunk
fn save_image(
    client:      &reqwest::blocking::Client,
    url:         &str,
    sess:        &str,
    out_dir:     &str,
    on_progress: impl Fn(u64, u64),
) -> Result<PathBuf, AnyError> {
    let hostname = url_hostname(url);
    let filename = url_filename(url);
    let cookie   = format!("AssetToken={sess}");

    fs::create_dir_all(out_dir)?;
    let filepath = Path::new(out_dir).join(filename);

    let mut resp = client
        .get(url)
        .header("Host", hostname)
        .header("Connection", "close")
        .header("User-Agent", "InternetRecovery/1.0")
        .header("Cookie", &cookie)
        .send()?;

    let total      = resp.content_length().unwrap_or(0);
    let mut file   = File::create(&filepath)?;
    let mut buf    = vec![0u8; 1 << 20]; // 1 MiB read buffer
    let mut downloaded = 0u64;

    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 { break; }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }

    Ok(filepath)
}


// chinklist verifir

/// im parsing the chunklist at `cnk_path` and verifying the RSA-2048/hash signature.

fn verify_chunklist(cnk_path: &Path) -> Result<Vec<(u32, [u8; 32])>, AnyError> {
    let data = fs::read(cnk_path)?;
    if data.len() < 36 {
        return Err("Chunklist file too small".into());
    }

    let magic            = &data[0..4];
    let header_size      = u32::from_le_bytes(data[4..8].try_into()?);
    let file_version     = data[8];
    let chunk_method     = data[9];
    let signature_method = data[10];
    // data[11] is padding
    let chunk_count      = u64::from_le_bytes(data[12..20].try_into()?);
    let chunk_offset     = u64::from_le_bytes(data[20..28].try_into()?);
    let signature_offset = u64::from_le_bytes(data[28..36].try_into()?);

    if magic != b"CNKL"                                   { return Err("Bad magic (not CNKL)".into()); }
    if header_size != 36                                   { return Err("Unexpected header size".into()); }
    if file_version != 1                                   { return Err("Unknown file version".into()); }
    if chunk_method != 1                                   { return Err("Unknown chunk method".into()); }
    if !matches!(signature_method, 1 | 2)                 { return Err("Unknown signature method".into()); }
    if chunk_count == 0                                    { return Err("Chunk count is zero".into()); }
    if chunk_offset != 0x24                                { return Err("Unexpected chunk offset".into()); }
    if signature_offset != 0x24 + 36 * chunk_count        { return Err("Unexpected signature offset".into()); }

    // hash header and chunk records
    let mut hasher = Sha256::new();
    hasher.update(&data[..36]);

    let mut chunks = Vec::with_capacity(chunk_count as usize);
    let mut pos    = 0x24usize;

    for _ in 0..chunk_count {
        if pos + 36 > data.len() { return Err("Truncated chunklist".into()); }
        hasher.update(&data[pos..pos + 36]);

        let chunk_size = u32::from_le_bytes(data[pos..pos + 4].try_into()?);
        let mut chunk_sha = [0u8; 32];
        chunk_sha.copy_from_slice(&data[pos + 4..pos + 36]);
        chunks.push((chunk_size, chunk_sha));
        pos += 36;
    }

    let digest = hasher.finalize();

    match signature_method {
        1 => {
            // rsa verification
            if pos + 256 > data.len() { return Err("Missing RSA signature bytes".into()); }
            let sig_bytes = &data[pos..pos + 256];

            let signature = BigUint::from_bytes_le(sig_bytes);
            let exponent  = BigUint::from(65537u32);
            let modulus   = BigUint::parse_bytes(APPLE_EFI_ROM_KEY_HEX.as_bytes(), 16)
                .ok_or("Failed to parse RSA modulus")?;

            let result = signature.modpow(&exponent, &modulus);

            let ff_part   = "f".repeat(404);
            let zero_part = "0".repeat(64);
            let template_hex = format!(
                "1{}003031300d060960864801650304020105000420{}",
                ff_part, zero_part
            );
            let template    = BigUint::parse_bytes(template_hex.as_bytes(), 16)
                .ok_or("Failed to parse RSA template")?;
            let digest_uint = BigUint::from_bytes_be(&digest);
            let expected    = template | digest_uint;

            if result != expected {
                return Err("RSA signature verification failed".into());
            }
        }
        2 => {
            if pos + 32 > data.len() { return Err("Missing hash signature bytes".into()); }
            if &data[pos..pos + 32] != digest.as_slice() {
                return Err("Hash verification failed".into());
            }
            return Err("Chunklist missing digital signature".into());
        }
        _ => unreachable!(),
    }

    Ok(chunks)
}

/// verify every chunk of `dmg_path` against the chunklist at `cnk_path`
fn verify_image(
    dmg_path:    &Path,
    cnk_path:    &Path,
    on_progress: impl Fn(usize, usize),
) -> Result<(), AnyError> {
    let chunks = verify_chunklist(cnk_path)?;
    let total  = chunks.len();
    let mut file = File::open(dmg_path)?;

    for (i, (chunk_size, expected_hash)) in chunks.iter().enumerate() {
        on_progress(i + 1, total);
        let mut buf = vec![0u8; *chunk_size as usize];
        let n = file.read(&mut buf)?;
        if n != *chunk_size as usize {
            return Err(format!("Chunk {} size mismatch: expected {}, got {}", i + 1, chunk_size, n).into());
        }
        let hash = Sha256::digest(&buf);
        if hash.as_slice() != expected_hash {
            return Err(format!("Chunk {} SHA-256 mismatch", i + 1).into());
        }
    }

    // make sure theres no extra data
    let mut extra = [0u8; 1];
    if file.read(&mut extra)? != 0 {
        return Err("Image is larger than chunklist expects".into());
    }

    Ok(())
}

// downloader on its own thread

fn log(prog: &SharedProgress, msg: impl Into<String>) {
    prog.lock().unwrap().log.push(msg.into());
}

fn set_phase(prog: &SharedProgress, phase: impl Into<String>) {
    prog.lock().unwrap().phase = phase.into();
}

fn do_download(version: &MacOSVersion, prog: &SharedProgress) -> Result<(), AnyError> {
    set_phase(prog, "Connecting to osrecovery.apple.com…");
    log(prog, "Building HTTP client…");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    set_phase(prog, "Fetching session cookie…");
    let session = get_session(&client)?;
    log(prog, "Session acquired.");

    set_phase(prog, "Requesting image info…");
    let info = get_image_info(&client, &session, version.build, version.model, version.os_type)?;
    log(prog, format!("Product: {}", info.get("AP").map(|s| s.as_str()).unwrap_or("?")));

    // chin list
    set_phase(prog, "Downloading chunklist…");
    let sign_link = info[INFO_SIGN_LINK].clone();
    let sign_sess = info[INFO_SIGN_SESS].clone();
    let prog_c    = prog.clone();

    let cnk_path = save_image(&client, &sign_link, &sign_sess, OUT_DIR, move |dl, tot| {
        let mut p = prog_c.lock().unwrap();
        p.cnk_downloaded = dl;
        p.cnk_total      = tot;
    })?;
    log(prog, format!("Chunklist saved → {}", cnk_path.display()));

    // damage file LMAO
    set_phase(prog, "Downloading BaseSystem.dmg…");
    let image_link = info[INFO_IMAGE_LINK].clone();
    let image_sess = info[INFO_IMAGE_SESS].clone();
    let prog_c     = prog.clone();

    let dmg_path = save_image(&client, &image_link, &image_sess, OUT_DIR, move |dl, tot| {
        let mut p = prog_c.lock().unwrap();
        p.dmg_downloaded = dl;
        p.dmg_total      = tot;
    })?;
    log(prog, format!("DMG saved → {}", dmg_path.display()));

    // verification process
    set_phase(prog, "Verifying image integrity…");
    let prog_c = prog.clone();
    verify_image(&dmg_path, &cnk_path, move |chunk, total| {
        let mut p = prog_c.lock().unwrap();
        p.verify_chunk = chunk;
        p.verify_total = total;
        p.phase        = format!("Verifying chunk {chunk}/{total}…");
    })?;
    log(prog, "✔  Image verified — all chunks match.");

    Ok(())
}

fn run_download(version: MacOSVersion, prog: SharedProgress) {
    match do_download(&version, &prog) {
        Ok(()) => {
            let mut p = prog.lock().unwrap();
            p.phase    = "✔  All operations completed successfully".to_string();
            p.finished = true;
        }
        Err(e) => {
            let mut p = prog.lock().unwrap();
            p.phase    = format!("✖  {e}");
            p.error    = Some(e.to_string());
            p.finished = true;
        }
    }
}


// tui app state

enum Screen {
    Menu,
    Downloading { version_name: String },
}

struct App {
    screen:           Screen,
    table_state:      TableState,
    versions:         Vec<MacOSVersion>,
    download_progress: Option<SharedProgress>,
}

impl App {
    fn new() -> Self {
        let mut ts = TableState::default();
        ts.select(Some(0));
        App {
            screen:            Screen::Menu,
            table_state:       ts,
            versions:          versions(),
            download_progress: None,
        }
    }

    fn cursor_up(&mut self) {
        let n = self.versions.len();
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some(if i == 0 { n - 1 } else { i - 1 }));
    }

    fn cursor_down(&mut self) {
        let n = self.versions.len();
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1) % n));
    }

    fn start_download(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            let version      = self.versions[idx].clone();
            let version_name = version.name.to_string();
            let progress     = Arc::new(Mutex::new(DownloadProgress::default()));
            let prog_clone   = progress.clone();

            thread::spawn(move || run_download(version, prog_clone));

            self.download_progress = Some(progress);
            self.screen = Screen::Downloading { version_name };
        }
    }

    fn is_finished(&self) -> bool {
        self.download_progress
            .as_ref()
            .map(|p| p.lock().unwrap().finished)
            .unwrap_or(false)
    }
}

// tui render

fn header_widget() -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("ocrecovery-rs", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("•", Style::default().fg(Color::Green)),
            Span::raw("  recovery for the 21st century"),
        ]),
        Line::from(Span::styled(
            "made with <3 by prodbyeternal",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Center)
}

fn render(frame: &mut Frame, app: &mut App) {
    match &app.screen {
        Screen::Menu => draw_menu(frame, app),
        Screen::Downloading { version_name } => {
            let name = version_name.clone();
            draw_download(frame, app, &name);
        }
    }
}

fn draw_menu(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    frame.render_widget(header_widget(), chunks[0]);

    let rows: Vec<Row> = app
        .versions
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Row::new(vec![
                Cell::from(format!("{:>2}", i + 1))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(v.name).style(Style::default().fg(Color::Cyan)),
                Cell::from(v.build).style(Style::default().fg(Color::Green)),
            ])
        })
        .collect();

    let widths = [Constraint::Length(4), Constraint::Length(20), Constraint::Min(30)];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["  #", "Version", "Board ID"])
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .title(" Available macOS Versions ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(table, chunks[1], &mut app.table_state);

    let footer = Paragraph::new(
        " ↑↓  Navigate   Enter  Select   q  Quit ",
    )
    .style(Style::default().fg(Color::DarkGray))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, chunks[2]);
}

fn pct(downloaded: u64, total: u64) -> u16 {
    if total == 0 { 0 } else { ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u16 }
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.2} GiB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.1} MiB", b as f64 / (1u64 << 20) as f64)
    } else if b >= 1 << 10 {
        format!("{:.1} KiB", b as f64 / (1u64 << 10) as f64)
    } else {
        format!("{b} B")
    }
}

fn draw_download(frame: &mut Frame, app: &App, version_name: &str) {
    let prog = match &app.download_progress {
        Some(p) => p.lock().unwrap().clone(),
        None    => return,
    };

    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // header
            Constraint::Length(3),  // version title
            Constraint::Length(3),  // chunklist gauge
            Constraint::Length(3),  // dmg gauge
            Constraint::Length(3),  // verify gauge
            Constraint::Length(3),  // status
            Constraint::Min(4),     // log
            Constraint::Length(3),  // footer
        ])
        .split(area);

    frame.render_widget(header_widget(), chunks[0]);

    // version thats downloaded
    let ver_para = Paragraph::new(format!(" Downloading: {version_name}"))
        .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );
    frame.render_widget(ver_para, chunks[1]);

    // chinklist gauge
    let cnk_label = if prog.cnk_total > 0 {
        format!("{} / {}", fmt_bytes(prog.cnk_downloaded), fmt_bytes(prog.cnk_total))
    } else {
        fmt_bytes(prog.cnk_downloaded)
    };
    let cnk_gauge = Gauge::default()
        .block(Block::default().title(" Chunklist ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .label(cnk_label)
        .percent(pct(prog.cnk_downloaded, prog.cnk_total));
    frame.render_widget(cnk_gauge, chunks[2]);

    // dmgg gauge
    let dmg_label = if prog.dmg_total > 0 {
        format!("{} / {}", fmt_bytes(prog.dmg_downloaded), fmt_bytes(prog.dmg_total))
    } else {
        fmt_bytes(prog.dmg_downloaded)
    };
    let dmg_gauge = Gauge::default()
        .block(Block::default().title(" BaseSystem.dmg ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .label(dmg_label)
        .percent(pct(prog.dmg_downloaded, prog.dmg_total));
    frame.render_widget(dmg_gauge, chunks[3]);

    // verification gauge
    let ver_label = if prog.verify_total > 0 {
        format!("Chunk {}/{}", prog.verify_chunk, prog.verify_total)
    } else {
        "—".to_string()
    };
    let ver_gauge = Gauge::default()
        .block(Block::default().title(" Verify Chunks ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
        .label(ver_label)
        .percent(if prog.verify_total > 0 {
            pct(prog.verify_chunk as u64, prog.verify_total as u64)
        } else {
            0
        });
    frame.render_widget(ver_gauge, chunks[4]);

    // status
    let (status_text, status_style) = if let Some(err) = &prog.error {
        (
            format!("✖  {err}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if prog.finished {
        (
            "✔  All operations completed successfully".to_string(),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            format!("⟳  {}", prog.phase),
            Style::default().fg(Color::Yellow),
        )
    };
    let status = Paragraph::new(status_text)
        .style(status_style)
        .alignment(Alignment::Center)
        .block(Block::default().title(" Status ").borders(Borders::ALL));
    frame.render_widget(status, chunks[5]);

    // log
    let log_items: Vec<ListItem> = prog
        .log
        .iter()
        .map(|s| ListItem::new(s.as_str()).style(Style::default().fg(Color::DarkGray)))
        .collect();
    let log_list = List::new(log_items).block(
        Block::default()
            .title(" Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(log_list, chunks[6]);

    // foot
    let footer_text = if prog.finished {
        " Press q to quit "
    } else {
        " Downloading — please wait… "
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    frame.render_widget(footer, chunks[7]);
}


// entry point

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        term.draw(|f| render(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match &app.screen {
                    Screen::Menu => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                        KeyCode::Up   | KeyCode::Char('k') => app.cursor_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.cursor_down(),
                        KeyCode::Enter                      => app.start_download(),
                        _ => {}
                    },
                    Screen::Downloading { .. } => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            if app.is_finished() {
                                break;
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
