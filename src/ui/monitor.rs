// htop-style TUI Process Monitor for Genshin-OS
use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};

use crate::hardware::Timer;
use crate::messaging::{FileRequest, KernelMsg, MemoryRequest, MessageBus, ProcessRequest, ResponseData};
use crate::ui::UIContext;

fn pid_color(n: u64) -> Color {
    match n % 8 {
        0 => Color::Green,
        1 => Color::Cyan,
        2 => Color::Magenta,
        3 => Color::Blue,
        4 => Color::Red,
        5 => Color::Yellow,
        6 => Color::LightRed,
        _ => Color::White,
    }
}

#[derive(Default, Clone)]
struct Snap {
    processes: Vec<String>,
    proc_count: usize,
    total_frames: u64,
    used_frames: u64,
    free_frames: u64,
    frame_map: Vec<(u64, u64)>,
    mem_ranges: Vec<String>,
    disk_total: u64,
    disk_used: u64,
    ticks: u64,
    uptime_secs: f64,
}

pub fn run_monitor(bus: Arc<dyn MessageBus>, timer: Arc<Timer>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let ctx = UIContext::new(bus);
    let mut snap = Snap::default();
    let res = run_loop(&mut terminal, &ctx, &timer, &mut snap);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    ctx: &UIContext,
    timer: &Arc<Timer>,
    snap: &mut Snap,
) -> io::Result<()> {
    loop {
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q' | 'Q') | KeyCode::Esc) {
                    break;
                }
            }
        }
        collect_snapshot(ctx, timer, snap);
        terminal.draw(|f| render(f, snap))?;
    }
    Ok(())
}

fn query(ctx: &UIContext, msg: KernelMsg) -> Option<crate::messaging::Response> {
    ctx.send_request(msg)
        .ok()
        .and_then(|rx| rx.recv_timeout(Duration::from_millis(100)).ok())
}

fn collect_snapshot(ctx: &UIContext, timer: &Arc<Timer>, snap: &mut Snap) {
    if let Some(resp) = query(ctx, KernelMsg::Process(ProcessRequest::GetStats)) {
        if let Some(ResponseData::StringList(procs)) = resp.data() {
            snap.processes = procs.clone();
            snap.proc_count = procs.len();
        }
    }
    if let Some(resp) = query(ctx, KernelMsg::Memory(MemoryRequest::GetStats)) {
        if let Some(ResponseData::MemoryStats { total_frames, used_frames, free_frames }) = resp.data() {
            snap.total_frames = *total_frames;
            snap.used_frames = *used_frames;
            snap.free_frames = *free_frames;
        }
    }
    if let Some(resp) = query(ctx, KernelMsg::Memory(MemoryRequest::GetFrameMap)) {
        if let Some(ResponseData::FrameMap(map)) = resp.data() {
            snap.frame_map = map.clone();
        }
    }
    if let Some(resp) = query(ctx, KernelMsg::Process(ProcessRequest::GetMemoryMap)) {
        if let Some(ResponseData::StringList(ranges)) = resp.data() {
            snap.mem_ranges = ranges.clone();
        }
    }
    if let Some(resp) = query(ctx, KernelMsg::File(FileRequest::DiskInfo)) {
        if let Some(ResponseData::DiskStats { total_sectors, used_sectors, total_bytes: _ }) = resp.data() {
            snap.disk_total = *total_sectors as u64;
            snap.disk_used = *used_sectors as u64;
        }
    }
    snap.ticks = timer.tick_count();
    snap.uptime_secs = snap.ticks as f64 * 0.01;
}

fn render(f: &mut Frame, snap: &Snap) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(9),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Header
    f.render_widget(
        Paragraph::new(format!(" Genshin-OS Monitor | Uptime: {:.1}s | Ticks: {} | Procs: {} | q=quit", snap.uptime_secs, snap.ticks, snap.proc_count))
            .block(Block::default().borders(Borders::ALL).style(Style::default().fg(Color::Cyan))),
        chunks[0],
    );

    // Process table
    let header = Row::new(vec![
        Cell::from("PID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("STATE").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("NAME").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("MEM").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let rows: Vec<Row> = snap.processes.iter().filter_map(|s| {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() < 3 { return None; }
        let st = parts.get(1).copied().unwrap_or("?");
        let style = match st {
            "Running" => Style::default().fg(Color::Green),
            "Blocked" => Style::default().fg(Color::Yellow),
            "Ready" => Style::default().fg(Color::Blue),
            "Zombie" => Style::default().fg(Color::Red),
            _ => Style::default(),
        };
        let pid_num: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let frames = snap.frame_map.iter().filter(|(_, o)| *o == pid_num).count();
        Some(Row::new(vec![
            Cell::from(pid_num.to_string()).style(style),
            Cell::from(st.to_string()).style(style),
            Cell::from((*parts.get(2).unwrap_or(&"?")).to_string()).style(style),
            Cell::from(format!("{}f", frames)),
        ]))
    }).collect();

    let widths = [Constraint::Length(6), Constraint::Length(10), Constraint::Length(22), Constraint::Length(8)];
    f.render_widget(
        Table::new(rows, widths).header(header).block(Block::default().title(" Processes ").borders(Borders::ALL).style(Style::default().fg(Color::Green))),
        chunks[1],
    );

    // Memory + Disk
    let md = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(chunks[2]);
    // Memory
    let mem_block = Block::default().title(" Memory ").borders(Borders::ALL).style(Style::default().fg(Color::Yellow));
    let mem_inner = mem_block.inner(md[0]);
    let mem_in = Layout::default().direction(Direction::Vertical).constraints([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(5),
    ]).split(mem_inner);
    f.render_widget(mem_block, md[0]);
    if snap.total_frames > 0 {
        let r = (snap.used_frames as f64 / snap.total_frames as f64).min(1.0);
        f.render_widget(
            Gauge::default().gauge_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)).ratio(r)
                .label(format!("{:.1}%  used={}KB  free={}KB  total={}KB", r*100.0, snap.used_frames*4, snap.free_frames*4, snap.total_frames*4)),
            mem_in[0],
        );

        if !snap.frame_map.is_empty() {
            let w = (mem_inner.width as usize).saturating_sub(2).max(40);
            let step = (snap.frame_map.len() / w).max(1);
            let mut spans: Vec<Span> = vec![Span::raw(" ")];
            let mut i = 0;
            while i < snap.frame_map.len() {
                let end = (i + step).min(snap.frame_map.len());
                let slice = &snap.frame_map[i..end];
                let owned_count = slice.iter().filter(|(_, o)| *o > 0).count();
                let ratio = owned_count as f64 / slice.len() as f64;
                let color = if ratio > 0.7 {
                    let first_owner = slice.iter().find(|(_, o)| *o > 0).map(|(_, o)| *o).unwrap_or(0);
                    pid_color(first_owner)
                } else {
                    Color::DarkGray
                };
                spans.push(Span::styled("\u{2588}", Style::default().fg(color)));
                i = end;
            }
            f.render_widget(Paragraph::new(Line::from(spans)), mem_in[1]);
        }

        // Per-process memory address ranges
        let mut lines = vec![format!("  Frames: {} used / {} free / {} total", snap.used_frames, snap.free_frames, snap.total_frames)];
        if snap.mem_ranges.is_empty() {
            lines.push("  (no frames allocated)".into());
        } else {
            for r in &snap.mem_ranges {
                lines.push(format!("  {}", r));
            }
        }
        f.render_widget(Paragraph::new(lines.join("\n")), mem_in[2]);
    }

    // Disk
    let disk_block = Block::default().title(" Disk ").borders(Borders::ALL).style(Style::default().fg(Color::Blue));
    let disk_in = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(3)]).split(disk_block.inner(md[1]));
    f.render_widget(disk_block, md[1]);
    if snap.disk_total > 0 {
        let r = (snap.disk_used as f64 / snap.disk_total as f64).min(1.0);
        f.render_widget(
            Gauge::default().gauge_style(Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)).ratio(r)
                .label(format!("{:.1}%  {} / {} sectors", r*100.0, snap.disk_used, snap.disk_total)),
            disk_in[0],
        );
        f.render_widget(Paragraph::new(format!("  .genshin-disk.img  |  Sector: 512B  |  {}KB used / {}KB total", snap.disk_used/2, snap.disk_total/2)), disk_in[1]);
    }

    // Footer
    f.render_widget(Paragraph::new(" htop for Genshin-OS | press q to quit ").style(Style::default().fg(Color::DarkGray)), chunks[3]);
}
